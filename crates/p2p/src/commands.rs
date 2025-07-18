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
        /// Libp2p [`PeerId`] of target peer.
        ///
        /// Note: [`PeerId`] can be created from public key of corresponding peer via
        /// `the_pubkey.to_peer_id()`.
        peer_id: PeerId,
        /// Message payload in raw bytes.
        ///
        /// The user is responsible for properly serializing/deserializing the data.
        data: Vec<u8>,
    },

    /// Connects to a peer, whitelists peer, and adds peer to the gossip sub network.
    ConnectToPeer(ConnectToPeerCommand),

    /// Directly queries P2P state (doesn't produce events).
    QueryP2PState(QueryP2PStateCommand),

    /// Actions related to filtering  a peer by its application [`PublicKey`].
    FilteringAction(FilteringActionCommand),
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

/// Command to do something with filtering peers by their application [`PublicKey`]
#[derive(Debug, Clone)]
pub enum FilteringActionCommand {
    /// i.e. if filtering is banlist, then ban
    /// if filtering is allowlist, then disallow
    /// if filtering is Scoring, then set some scores to some low values.
    DisrespectAppPkToCloseConnection {
        /// Application [`PublicKey`] of the peer to disrespect
        app_pk: PublicKey,
    },

    /// i.e. if filtering is banlist, then unban
    /// if filtering is allowlist, then disallow
    /// if filtering is Scoring, then set some scores to some high values.
    RespectAppPkToAllowConnection {
        /// Application [`PublicKey`] of the peer to disrespect
        app_pk: PublicKey,
    },
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
        /// Peer ID to get the app public key for.
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
