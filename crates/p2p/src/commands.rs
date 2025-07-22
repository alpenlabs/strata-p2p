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
        /// Libp2p application [`PublicKey`] of target peer.
        app_pk: PublicKey,
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
    /// Transport ID.
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
        /// Transport ID to check.
        peer_id: PeerId,
        /// Channel to send the response back.
        response_sender: oneshot::Sender<bool>,
    },

    /// Gets all connected peers.
    GetConnectedPeers {
        /// Channel to send the response back.
        response_sender: oneshot::Sender<Vec<PeerId>>,
    },

    /// Gets all listening addresses from swarm's point of view.
    /// May give empty [`Vec`] if transport initialization has not yet occurred at the moment of the
    /// call.
    GetMyListeningAddresses {
        /// Channel to send the response back.
        response_sender: oneshot::Sender<Vec<Multiaddr>>,
    },

    /// Gets the app public key for a specific peer.
    /// Returns None if we are not connected to the peer or key exchange hasn't happened yet.
    GetAppPublicKey {
        /// Transport ID to get the app public key for.
        peer_id: PeerId,
        /// Channel to send the response back.
        response_sender: oneshot::Sender<Option<PublicKey>>,
    },
}

impl From<QueryP2PStateCommand> for Command {
    fn from(v: QueryP2PStateCommand) -> Self {
        Self::QueryP2PState(v)
    }
}
