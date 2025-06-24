//! Commands for P2P implementation from operator implementation.

use libp2p::{Multiaddr, PeerId, identity::PublicKey};
use tokio::sync::oneshot;

/// Commands that users can send to the P2P node.
#[derive(Debug)]
pub enum Command {
    /// Publishes message through gossip sub network of peers.
    PublishMessage {
        /// Message payload in raw bytes.
        ///
        /// The user is responsible for properly serializing/deserializing the data.
        data: Vec<u8>,
    },

    /// Requests some message directly from other operator by public Key.
    RequestMessage {
        /// Libp2p public key of target peer.
        ///
        /// Note: public key type (secp256k1/ed25519/RSA) is set via enabling specific feature in
        /// cargo toml for libp2p.
        peer_pubkey: PublicKey,
        /// Message payload in raw bytes.
        ///
        /// The user is responsible for properly serializing/deserializing the data.
        data: Vec<u8>,
    },

    /// Connects to a peer, whitelists peer, and adds peer to the gossip sub network.
    ConnectToPeer(ConnectToPeerCommand),

    /// Directly queries P2P state (doesn't produce events).
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

/// Commands to directly query P2P state information.
#[derive(Debug)]
pub enum QueryP2PStateCommand {
    /// Queries if we're connected to a specific peer
    IsConnected {
        /// Peer ID to check.
        peer_id: PeerId,
        /// Channel to send the response back.
        response_sender: oneshot::Sender<bool>,
    },

    /// Gets all connected peers.
    GetConnectedPeers {
        /// Channel to send the response back.
        response_sender: oneshot::Sender<Vec<PeerId>>,
    },
}

impl From<QueryP2PStateCommand> for Command {
    fn from(v: QueryP2PStateCommand) -> Self {
        Self::QueryP2PState(v)
    }
}
