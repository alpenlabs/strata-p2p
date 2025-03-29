//! Commands for P2P implementation from operator implementation.

use bitcoin::{hashes::sha256, Txid, XOnlyPublicKey};
use libp2p::{identity::secp256k1, Multiaddr, PeerId};
use musig2::{PartialSignature, PubNonce};
use strata_p2p_types::{P2POperatorPubKey, Scope, SessionId, StakeChainId, WotsPublicKeys};
use strata_p2p_wire::p2p::v1::{GetMessageRequest, GossipsubMsg, UnsignedGossipsubMsg};
use tokio::sync::oneshot;

/// Ask P2P implementation to distribute some data across network.
#[derive(Debug)]
#[expect(clippy::large_enum_variant)]
pub enum Command {
    /// Publishes message through gossip sub network of peers.
    PublishMessage(PublishMessage),

    /// Requests some message directly from other operator by peer id.
    RequestMessage(GetMessageRequest),

    /// Connects to a peer, whitelists peer, and adds peer to the gossip sub network.
    ConnectToPeer(ConnectToPeerCommand),

    /// Directly queries P2P state (doesn't produce events)
    QueryP2PState(QueryP2PStateCommand),
}

#[derive(Debug, Clone)]
pub struct PublishMessage {
    /// Operator's P2P public key.
    pub key: P2POperatorPubKey,

    /// Operator's signature over the message.
    pub signature: Vec<u8>,

    /// Unsigned message.
    pub msg: UnsignedPublishMessage,
}

/// Types of unsigned messages.
#[derive(Debug, Clone)]
#[expect(clippy::large_enum_variant)]
pub enum UnsignedPublishMessage {
    /// Stake Chain information.
    StakeChainExchange {
        /// 32-byte hash of some unique to stake chain data.
        stake_chain_id: StakeChainId,

        /// [`Txid`] of the pre-stake transaction.
        pre_stake_txid: Txid,

        /// vout index of the pre-stake transaction.
        pre_stake_vout: u32,
    },

    /// Deposit setup.
    ///
    /// Primarily used for the WOTS PKs.
    DepositSetup {
        /// The deposit [`Scope`].
        scope: Scope,

        /// Index of the deposit.
        index: u32,

        /// [`sha256::Hash`] hash of the stake transaction that the preimage is revealed when
        /// advancing the stake.
        hash: sha256::Hash,

        /// Funding transaction ID.
        ///
        /// Used to cover the dust outputs in the transaction graph connectors.
        funding_txid: Txid,

        /// Funding transaction output index.
        ///
        /// Used to cover the dust outputs in the transaction graph connectors.
        funding_vout: u32,

        /// Operator's X-only public key to construct a P2TR address to reimburse the
        /// operator for a valid withdraw fulfillment.
        // TODO: convert this a BOSD descriptor.
        operator_pk: XOnlyPublicKey,

        /// Winternitz One-Time Signature (WOTS) public keys shared in a deposit.
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
                pre_stake_txid,
                pre_stake_vout,
            } => UnsignedGossipsubMsg::StakeChainExchange {
                stake_chain_id,
                pre_stake_txid,
                pre_stake_vout,
            },

            UnsignedPublishMessage::DepositSetup {
                scope,
                index,
                hash,
                funding_txid,
                funding_vout,
                operator_pk,
                wots_pks,
            } => UnsignedGossipsubMsg::DepositSetup {
                scope,
                index,
                hash,
                funding_txid,
                funding_vout,
                operator_pk,
                wots_pks,
            },

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

/// Connects to a peer, whitelists peer, and adds peer to the gossip sub network.
#[derive(Debug, Clone)]
pub struct ConnectToPeerCommand {
    /// Peer ID.
    pub peer_id: PeerId,

    /// Peer address.
    pub peer_addr: Multiaddr,
}

impl From<ConnectToPeerCommand> for Command {
    fn from(v: ConnectToPeerCommand) -> Self {
        Self::ConnectToPeer(v)
    }
}

/// Commands to directly query P2P state information
#[derive(Debug)]
pub enum QueryP2PStateCommand {
    /// Query if we're connected to a specific peer
    IsConnected {
        /// Peer ID to check
        peer_id: PeerId,
        /// Channel to send the response back
        response_sender: oneshot::Sender<bool>,
    },

    /// Get all connected peers
    GetConnectedPeers {
        /// Channel to send the response back
        response_sender: oneshot::Sender<Vec<PeerId>>,
    },
}

impl From<QueryP2PStateCommand> for Command {
    fn from(v: QueryP2PStateCommand) -> Self {
        Self::QueryP2PState(v)
    }
}
