//! Commands for P2P implementation from operator implementation.

use libp2p::{Multiaddr, identity::PublicKey};
use tokio::sync::oneshot;

#[cfg(feature = "kad")]
use crate::swarm::dto::dht_record::RecordData;

/// Commands that users can send to the P2P node.
#[derive(Debug)]
pub enum Command {
    /// Dials a set of address directly.
    ConnectToPeer {
        /// Application public key to associate with the dial sequence.
        app_public_key: PublicKey,

        /// List of multiaddresses to try dialing.
        addresses: Vec<Multiaddr>,
    },

    /// Disconnects from a peer.
    DisconnectFromPeer {
        /// Libp2p [`PublicKey`] of target peer.
        target_app_public_key: PublicKey,
    },

    /// Directly queries P2P state (doesn't produce events).
    QueryP2PState(QueryP2PStateCommand),

    /// Try get record in DHT where application public is a key. A record is a [`RecordData`].
    /// Checking of signature is enabled and works via a Signer.
    #[cfg(feature = "kad")]
    GetDHTRecord {
        /// Key for DHT record: a remote node's application public key.
        app_public_key: PublicKey,
        /// One shot channel for sending result back.
        response_sender: tokio::sync::oneshot::Sender<Option<RecordData>>,
    },
}

/// Command to publish a message through gossipsub.
#[cfg(feature = "gossipsub")]
#[derive(Debug)]
pub struct GossipCommand {
    /// Message payload in raw bytes.
    ///
    /// The user is responsible for properly serializing/deserializing the data.
    pub data: Vec<u8>,
}

/// Command to request a message from a specific peer.
#[cfg(feature = "request-response")]
#[derive(Debug)]
pub struct RequestResponseCommand {
    /// Libp2p [`PublicKey`] of target peer.
    pub target_app_public_key: PublicKey,

    /// Message payload in raw bytes.
    ///
    /// The user is responsible for properly serializing/deserializing the data.
    pub data: Vec<u8>,
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
        response_sender: oneshot::Sender<Vec<PublicKey>>,
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
