//! Event types for the setup protocol.
//!
//! This module defines the events that are emitted during the setup phase
//! of peer-to-peer connections, providing information about handshake progress
//! and key exchange completion.

use libp2p::{PeerId, identity::ed25519::PublicKey};

/// Events emitted during the setup phase of peer connections.
///
/// These events provide feedback on the progress of the handshake protocol
/// and are used to communicate setup milestones to the swarm.
#[derive(Debug, Clone)]
pub enum SetupEvent {
    /// Indicates that an application public key has been received from a peer.
    ///
    /// This event is fired when the local node successfully receives and
    /// processes a peer's application public key during the handshake.
    AppKeyReceived {
        peer_id: PeerId,
        app_public_key: PublicKey,
    },
    /// Indicates that the handshake process has completed successfully.
    ///
    /// This event is fired when the entire handshake protocol has finished,
    /// signifying that the connection is ready for application-level communication.
    HandshakeComplete {
        peer_id: PeerId,
    },
}
