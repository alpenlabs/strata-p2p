//! Commands for P2P implementation from operator implementation.

use bitcoin::{OutPoint, XOnlyPublicKey};
use libp2p::{identity::secp256k1, Multiaddr, PeerId};
use musig2::{PartialSignature, PubNonce};
use strata_p2p_types::{OperatorPubKey, Scope, SessionId, StakeChainId, StakeData, WotsPublicKeys};
use strata_p2p_wire::p2p::v1::{
    GetMessageRequest, GossipsubMsg, StakeChainExchange, UnsignedGossipsubMsg,
};

/// Ask P2P implementation to distribute some data across network.
#[derive(Debug, Clone)]
pub enum Command {
    /// Publishes message through gossip sub network of peers.
    PublishMessage(PublishMessage),

    /// Requests some message directly from other operator by peer id.
    RequestMessage(GetMessageRequest),

    /// Cleans session, scopes from internal DB.
    CleanStorage(CleanStorageCommand),

    /// Connects to a peer, whitelists peer, and adds peer to the swarm.
    ConnectToPeer(ConnectToPeerCommand),
}

#[derive(Debug, Clone)]
pub struct PublishMessage {
    /// Operator's public key.
    pub key: OperatorPubKey,

    /// Operator's signature over the message.
    pub signature: Vec<u8>,

    /// Unsigned message.
    pub msg: UnsignedPublishMessage,
}

/// Types of unsigned messages.
#[derive(Debug, Clone)]
pub enum UnsignedPublishMessage {
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
        wots_pks: WotsPublicKeys,
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

impl From<PublishMessage> for GossipsubMsg {
    /// Converts [`PublishMessage`] into [`GossipsubMsg`].
    fn from(value: PublishMessage) -> Self {
        GossipsubMsg {
            signature: value.signature,
            key: value.key,
            unsigned: value.msg.into(),
        }
    }
}

impl UnsignedPublishMessage {
    /// Signs `self` using supplied [`secp256k1::Keypair`]. Returns a `Command`
    /// with resulting signature and public key from [`secp256k1::Keypair`].
    pub fn sign_secp256k1(&self, keypair: &secp256k1::Keypair) -> PublishMessage {
        let kind: UnsignedGossipsubMsg = self.clone().into();
        let msg = kind.content();
        let signature = keypair.secret().sign(&msg);

        PublishMessage {
            key: keypair.public().clone().into(),
            signature,
            msg: self.clone(),
        }
    }
}

impl From<UnsignedPublishMessage> for UnsignedGossipsubMsg {
    /// Converts [`UnsignedPublishMessage`] into [`UnsignedGossipsubMsg`].
    fn from(value: UnsignedPublishMessage) -> Self {
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

            UnsignedPublishMessage::DepositSetup { scope, wots_pks } => {
                UnsignedGossipsubMsg::DepositSetup { scope, wots_pks }
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

impl From<PublishMessage> for Command {
    fn from(v: PublishMessage) -> Self {
        Self::PublishMessage(v)
    }
}

/// Connects to a peer.
#[derive(Debug, Clone)]
pub struct ConnectToPeerCommand {
    /// Peer ID.
    pub peer_id: PeerId,

    /// Peer address.
    pub peer_addr: Multiaddr,
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

impl From<CleanStorageCommand> for Command {
    fn from(v: CleanStorageCommand) -> Self {
        Self::CleanStorage(v)
    }
}
