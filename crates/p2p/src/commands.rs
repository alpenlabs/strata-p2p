//! Commands for P2P implementation from operator implementation.

use bitcoin::{OutPoint, XOnlyPublicKey};
use libp2p::identity::secp256k1;
use musig2::{PartialSignature, PubNonce};
use prost::Message;
use strata_p2p_types::{OperatorPubKey, Scope, SessionId};
use strata_p2p_wire::p2p::v1::{
    DepositSetup, GenesisInfo, GetMessageRequest, GossipsubMsg, UnsignedGossipsubMsg,
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
    /// Genesis information.
    ///
    /// Primarily used for the Stake Chain setup.
    GenesisInfo {
        pre_stake_outpoint: OutPoint,
        checkpoint_pubkeys: Vec<XOnlyPublicKey>,
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
        let kind: UnsignedGossipsubMsg<DSP> = self.clone().into();
        let msg = kind.content();
        let signature = keypair.secret().sign(&msg);

        PublishMessage {
            key: keypair.public().clone().into(),
            signature,
            msg: self.clone(),
        }
    }
}

impl<DSP: Message + Clone> From<UnsignedPublishMessage<DSP>> for UnsignedGossipsubMsg<DSP> {
    /// Converts [`UnsignedPublishMessage`] into [`UnsignedGossipsubMsg`].
    fn from(value: UnsignedPublishMessage<DSP>) -> Self {
        match value {
            UnsignedPublishMessage::GenesisInfo {
                pre_stake_outpoint,
                checkpoint_pubkeys,
            } => UnsignedGossipsubMsg::GenesisInfo(GenesisInfo {
                checkpoint_pubkeys,
                pre_stake_outpoint,
            }),

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
