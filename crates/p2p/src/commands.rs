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
        app_public_key: PublicKey,
        /// Message payload in raw bytes.
        ///
        /// The user is responsible for properly serializing/deserializing the data.
        data: Vec<u8>,
    },

    /// Dials a set of address directly.
    ConnectToPeer {
        /// Application public key to associate with the dial sequence.
        app_public_key: PublicKey,
        /// List of multiaddresses to try dialing.
        addresses: Vec<Multiaddr>,
    },

    /// Directly queries P2P state (doesn't produce events).
    QueryP2PState(QueryP2PStateCommand),
}

/// Commands to directly query P2P state information.
#[derive(Debug)]
pub enum QueryP2PStateCommand {
    /// Queries if we're connected to a specific peer
    IsConnected {
        /// App public key to check.
        app_public_key: PublicKey,
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
}

impl From<QueryP2PStateCommand> for Command {
    fn from(v: QueryP2PStateCommand) -> Self {
        Self::QueryP2PState(v)
    }
}
