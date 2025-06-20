//! Commands for P2P implementation from operator implementation.

use bitcoin::{hashes::sha256, Txid, XOnlyPublicKey};
use libp2p::{identity::secp256k1, Multiaddr, PeerId};
use strata_p2p_types::{P2POperatorPubKey, Scope, SessionId};
use tokio::sync::oneshot;

/// Ask P2P implementation to distribute some data across network.
#[derive(Debug)]
#[expect(clippy::large_enum_variant)]
pub enum Command {
    /// Publishes message through gossip sub network of peers.
    PublishMessage { data: Vec<u8> },

    /// Requests some message directly from other operator by peer id.
    RequestMessage {
        peer_pubkey: P2POperatorPubKey,
        data: Vec<u8>,
    },

    /// Connects to a peer, whitelists peer, and adds peer to the gossip sub network.
    ConnectToPeer(ConnectToPeerCommand),

    /// Directly queries P2P state (doesn't produce events)
    QueryP2PState(QueryP2PStateCommand),
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
