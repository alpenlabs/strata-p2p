//! Commands for P2P implementation from operator implementation.

use libp2p::Multiaddr;
#[cfg(not(feature = "byos"))]
use libp2p::PeerId;
#[cfg(feature = "byos")]
use libp2p::identity::PublicKey;
use tokio::sync::oneshot;

#[cfg(all(
    any(feature = "gossipsub", feature = "request-response"),
    not(feature = "byos")
))]
use crate::score_manager::PeerScore;

/// Commands that users can send to the P2P node.
#[derive(Debug)]
pub enum Command {
    /// Dials a set of address directly.
    ConnectToPeer {
        #[cfg(feature = "byos")]
        /// Application public key to associate with the dial sequence.
        app_public_key: PublicKey,

        #[cfg(not(feature = "byos"))]
        /// Transport peer ID to associate with the dial sequence.
        transport_id: PeerId,

        /// List of multiaddresses to try dialing.
        addresses: Vec<Multiaddr>,
    },

    /// Disconnects from a peer.
    DisconnectFromPeer {
        #[cfg(feature = "byos")]
        /// Libp2p [`PublicKey`] of target peer.
        target_app_public_key: PublicKey,

        #[cfg(not(feature = "byos"))]
        /// Libp2p [`PeerId`] of target peer.
        target_transport_id: PeerId,
    },

    /// Gets [`PeerScore`] for a specific peer by [`PeerId`].
    #[cfg(all(
        any(feature = "gossipsub", feature = "request-response"),
        not(feature = "byos")
    ))]
    GetPeerScore {
        /// Transport `PeerId` to query.
        peer_id: PeerId,
        /// Channel to send the response back.
        response_sender: oneshot::Sender<PeerScore>,
    },

    /// Directly queries P2P state (doesn't produce events).
    QueryP2PState(QueryP2PStateCommand),
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
    #[cfg(feature = "byos")]
    /// Libp2p [`PublicKey`] of target peer.
    pub target_app_public_key: PublicKey,

    #[cfg(not(feature = "byos"))]
    /// Libp2p [`PeerId`] of target peer.
    pub target_transport_id: PeerId,

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
        #[cfg(feature = "byos")]
        /// App public key to check.
        app_public_key: PublicKey,

        #[cfg(not(feature = "byos"))]
        /// Transport peer ID to check.
        transport_id: PeerId,

        /// Channel to send the response back.
        response_sender: oneshot::Sender<bool>,
    },

    /// Gets all connected peers.
    GetConnectedPeers {
        #[cfg(feature = "byos")]
        /// Channel to send the response back.
        response_sender: oneshot::Sender<Vec<PublicKey>>,

        #[cfg(not(feature = "byos"))]
        /// Channel to send the response back.
        response_sender: oneshot::Sender<Vec<PeerId>>,
    },

    /// Gets all listening addresses from swarm's point of view.
    /// May give empty [`Vec`] if transport initialization has not yet occurred at the moment of
    /// the call.
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
