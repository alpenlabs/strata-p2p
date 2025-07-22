//! Event types for the setup protocol.
//!
//! This module defines the events that are emitted during the setup phase
//! of peer-to-peer connections, providing information about handshake progress
//! and key exchange completion.

use libp2p::{
    PeerId,
    identity::{DecodingError, PublicKey},
    swarm::StreamUpgradeError,
};

/// Variations of error during a handshake.
#[derive(Debug)]
pub enum ErrorDuringHandshakeVariations {
    /// Indicates that signature verification failed.
    ///
    /// This event is fired when the signature verification fails for a peer's
    /// handshake message, indicating the connection should be dropped.
    SignatureVerificationFailed,

    /// Failed to deserialize something.
    DeserializationFailed(serde_json::Error),

    /// In received message application public key is invalid.
    AppPkInvalid(DecodingError),

    /// Error during sending to remote peer.
    OutboundError(StreamUpgradeError<tokio::io::Error>),

    /// Error during receiving from remote peer.
    InboundError(tokio::io::Error),
}

/// Events emitted during the setup phase of peer connections.
///
/// These events provide feedback on the progress of the handshake protocol
/// and are used to communicate setup milestones to the swarm.
#[derive(Debug)]
pub enum SetupBehaviourEvent {
    /// Indicates that an application public key has been received from a peer.
    ///
    /// This event is fired when the local node successfully receives and
    /// processes a peer's application public key during the handshake.
    AppKeyReceived {
        transport_id: PeerId,
        app_public_key: PublicKey,
    },
    /// Indicates that the handshake process has completed successfully.
    ///
    /// This event is fired when the entire handshake protocol has finished,
    /// signifying that the connection is ready for application-level communication.
    HandshakeComplete { transport_id: PeerId },

    /// Indicates that signature verification failed.
    ///
    /// This event is fired when the signature verification fails for a peer's
    /// handshake message, indicating the connection should be dropped.
    ErrorDuringHandshake {
        transport_id: PeerId,
        error: ErrorDuringHandshakeVariations,
    },
}

/// Events emitted during the setup phase of peer connections.
///
/// These events provide feedback on the progress of the handshake protocol
/// and are used to communicate setup milestones to the swarm.
#[derive(Debug)]
pub enum SetupHandlerEvent {
    /// Indicates that an application public key has been received from a peer.
    ///
    /// This event is fired when the local node successfully receives and
    /// processes a peer's application public key during the handshake.
    AppKeyReceived { app_public_key: PublicKey },
    /// Indicates that the handshake process has completed successfully.
    ///
    /// This event is fired when the entire handshake protocol has finished,
    /// signifying that the connection is ready for application-level communication.
    HandshakeComplete,

    /// Something has failed during handshake.
    ErrorDuringHandshake(ErrorDuringHandshakeVariations),
}
