//! Commands for P2P implementation from operator implementation.

use libp2p::{Multiaddr, PeerId};
use tokio::sync::oneshot;

use crate::operator_pubkey::P2POperatorPubKey;

/// Ask P2P implementation to distribute some data across network.
#[derive(Debug)]
pub enum Command {
    /// Publishes message through gossip sub network of peers.
    PublishMessage { 
        /// Just bytes.
        data: Vec<u8> 
    },

    /// Requests some message directly from other operator by peer id.
    RequestMessage {
        /// A wrapper around libp2p::ed25519::PublicKey
        peer_pubkey: P2POperatorPubKey,
        /// Bytes. Expected to be parsed, validated by users of the lib.
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
