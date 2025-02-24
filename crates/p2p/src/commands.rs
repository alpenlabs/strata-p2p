//! Commands for P2P implementation from operator implementation.

use bitcoin::{
    hashes::{sha256, Hash},
    OutPoint, XOnlyPublicKey,
};
use musig2::{PartialSignature, PubNonce};
use prost::Message;
use secp256k1::{rand::thread_rng, Message as SecpMessage, SECP256K1};
use strata_p2p_types::{OperatorPubKey, Scope, SessionId, StakeChainId, StakeData};
use strata_p2p_wire::p2p::v1::{
    DepositSetup, GetMessageRequest, GossipsubMsg, StakeChainExchange, UnsignedGossipsubMsg,
};

/// Ask P2P implementation to distribute some data across network.
#[derive(Debug, Clone)]
pub enum Command<DepositSetupPayload> {
    /// Publishes message through gossip sub network of peers.
    PublishMessage(PublishMessage<DepositSetupPayload>),

    /// Requests some message directly from other operator by peer id.
    RequestMessage(GetMessageRequest),

    /// Cleans session, scopes from internal DB.
    CleanStorage(CleanStorageCommand),
}

#[derive(Debug, Clone)]
pub struct PublishMessage<DepositSetupPayload> {
    /// Operator's public key.
    pub key: OperatorPubKey,

    /// Operator's signature over the message.
    pub signature: Vec<u8>,

    /// Unsigned message.
    pub msg: UnsignedPublishMessage<DepositSetupPayload>,
}

/// Types of unsigned messages.
#[derive(Debug, Clone)]
pub enum UnsignedPublishMessage<DepositSetupPayload> {
    /// Stake Chain information.
    StakeChainExchange {
        /// 32-byte hash of some unique to stake chain data.
        stake_chain_id: StakeChainId,

        /// [`OutPoint`] of the pre-stake transaction.
        pre_stake_outpoint: OutPoint,

        /// Each operator `i = 0..N` sends a message with his Schnorr verification keys `Y_{i,j}`
        /// for blocks `j = 0..M`.
        checkpoint_pubkeys: Vec<XOnlyPublicKey>,

        /// Stake data for a whole Stake Chain.
        stake_data: Vec<StakeData>,
    },

    /// Deposit setup.
    ///
    /// Primarily used for the WOTS PKs.
    DepositSetup {
        /// The deposit [`Scope`].
        scope: Scope,

        /// Payload, WOTS PKs.
        payload: DepositSetupPayload,
    },

    /// MuSig2 (public) nonces exchange.
    Musig2NoncesExchange {
        /// The [`SessionId`].
        session_id: SessionId,

        /// Payload, (public) nonces.
        pub_nonces: Vec<PubNonce>,
    },

    /// MuSig2 (partial) signatures exchange.
    Musig2SignaturesExchange {
        /// The [`SessionId`].
        session_id: SessionId,

        /// Payload, (partial) signatures.
        partial_sigs: Vec<PartialSignature>,
    },
}

impl<DSP: Message + Clone> From<PublishMessage<DSP>> for GossipsubMsg<DSP> {
    /// Converts [`PublishMessage`] into [`GossipsubMsg`].
    fn from(value: PublishMessage<DSP>) -> Self {
        GossipsubMsg {
            signature: value.signature,
            key: value.key,
            unsigned: value.msg.into(),
        }
    }
}

impl<DSP: Message + Default + Clone> UnsignedPublishMessage<DSP> {
    /// Signs `self` using supplied [`secp256k1::Keypair`]. Returns a `Command`
    /// with resulting signature and public key from [`secp256k1::Keypair`].
    pub fn sign_secp256k1(&self, keypair: &secp256k1::Keypair) -> PublishMessage<DSP> {
        let mut rng = thread_rng();
        let (key, _) = keypair.public_key().x_only_public_key().into();
        let kind: UnsignedGossipsubMsg<DSP> = self.clone().into();
        let content = kind.content();
        let digest = sha256::Hash::const_hash(&content);
        let digest = digest.as_byte_array();
        let message = SecpMessage::from_digest(*digest);
        let signature = SECP256K1
            .sign_schnorr_with_rng(&message, keypair, &mut rng)
            .serialize()
            .to_vec();

        PublishMessage {
            key: key.into(),
            signature,
            msg: self.clone(),
        }
    }
}

impl<DSP: Message + Clone> From<UnsignedPublishMessage<DSP>> for UnsignedGossipsubMsg<DSP> {
    /// Converts [`UnsignedPublishMessage`] into [`UnsignedGossipsubMsg`].
    fn from(value: UnsignedPublishMessage<DSP>) -> Self {
        match value {
            UnsignedPublishMessage::StakeChainExchange {
                stake_chain_id,
                pre_stake_outpoint,
                checkpoint_pubkeys,
                stake_data,
            } => UnsignedGossipsubMsg::StakeChainExchange {
                stake_chain_id,
                info: StakeChainExchange {
                    checkpoint_pubkeys,
                    pre_stake_outpoint,
                    stake_data,
                },
            },

            UnsignedPublishMessage::DepositSetup { scope, payload } => {
                UnsignedGossipsubMsg::DepositSetup {
                    scope,
                    setup: DepositSetup { payload },
                }
            }

            UnsignedPublishMessage::Musig2NoncesExchange {
                session_id,
                pub_nonces,
            } => UnsignedGossipsubMsg::Musig2NoncesExchange {
                session_id,
                nonces: pub_nonces,
            },

            UnsignedPublishMessage::Musig2SignaturesExchange {
                session_id,
                partial_sigs,
            } => UnsignedGossipsubMsg::Musig2SignaturesExchange {
                session_id,
                signatures: partial_sigs,
            },
        }
    }
}

impl<DepositSetupPayload> From<PublishMessage<DepositSetupPayload>>
    for Command<DepositSetupPayload>
{
    fn from(v: PublishMessage<DepositSetupPayload>) -> Self {
        Self::PublishMessage(v)
    }
}

/// Commands P2P to clean entries from internal key-value storage by
/// session IDs, scopes and operator pubkeys.
#[derive(Debug, Clone)]
pub struct CleanStorageCommand {
    /// [`Scope`]s to clean.
    pub scopes: Vec<Scope>,

    /// [`SessionId`]s to clean.
    pub session_ids: Vec<SessionId>,

    /// [`OperatorPubKey`]s to clean.
    pub operators: Vec<OperatorPubKey>,
}

impl CleanStorageCommand {
    /// Creates a new [`CleanStorageCommand`].
    pub const fn new(
        scopes: Vec<Scope>,
        session_ids: Vec<SessionId>,
        operators: Vec<OperatorPubKey>,
    ) -> Self {
        Self {
            scopes,
            session_ids,
            operators,
        }
    }

    /// Clean entries only by [`Scope`] and [`OperatorPubKey`]s from storage.
    pub const fn with_scopes(scopes: Vec<Scope>, operators: Vec<OperatorPubKey>) -> Self {
        Self {
            scopes,
            session_ids: Vec::new(),
            operators,
        }
    }

    /// Clean entries only by [`SessionId`]s and [`OperatorPubKey`]s from storage.
    pub const fn with_session_ids(
        session_ids: Vec<SessionId>,
        operators: Vec<OperatorPubKey>,
    ) -> Self {
        Self {
            scopes: Vec::new(),
            session_ids,
            operators,
        }
    }
}

impl<DSP> From<CleanStorageCommand> for Command<DSP> {
    fn from(v: CleanStorageCommand) -> Self {
        Self::CleanStorage(v)
    }
}
