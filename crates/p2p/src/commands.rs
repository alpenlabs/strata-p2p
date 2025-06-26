//! Commands for P2P implementation from operator implementation.

#[cfg(test)]
use libp2p::build_multiaddr;
use libp2p::{Multiaddr, PeerId};
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_connect_to_peer() {
        let tmp = ConnectToPeerCommand {
            peer_id: libp2p::identity::Keypair::generate_ed25519()
                .public()
                .to_peer_id(),
            peer_addr: build_multiaddr!(Memory(1_u64)),
        };
        let _ = Command::from(tmp);
    }

    #[test]
    fn test_from_query_p2p_state() {
        let (tx, _rx) = oneshot::channel::<bool>();

        let tmp = QueryP2PStateCommand::IsConnected {
            peer_id: libp2p::identity::Keypair::generate_ed25519()
                .public()
                .to_peer_id(),
            response_sender: tx,
        };
        let _ = Command::from(tmp);
    }
}
