//! Swarm errors.

use std::io;

use libp2p::TransportError;
use thiserror::Error;

/// P2P result type.
pub type P2PResult<T> = Result<T, Error>;

/// Swarm errors.
#[derive(Debug, Error)]
pub enum Error {
<<<<<<< HEAD
    // Validation errors.
=======
    /// Validation errors.
>>>>>>> dev-v2
    #[error("Validation error {0}")]
    Validation(#[from] ValidationError),

    /// Protocol errors.
    #[error("Protocol error {0}")]
    Protocol(#[from] ProtocolError),
}

/// Validation errors.
#[derive(Debug, Error)]
pub enum ValidationError {
<<<<<<< HEAD
    #[error("Invalid signature")]
    InvalidSignature,
=======
    /// The signature is invalid.
    #[error("Invalid signature")]
    InvalidSignature,
    /// The message signer is not in the signer's allowlist.
>>>>>>> dev-v2
    #[error("Not in signers allowlist")]
    NotInSignersAllowlist,
}

<<<<<<< HEAD
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Failed to listen: {0}")]
    Listen(#[from] TransportError<io::Error>),

    #[error("Events channel closed: {0}")]
    EventsChannelClosed(Box<dyn std::error::Error + Sync + Send>),

    #[error("Failed to initialize transport: {0}")]
    TransportInitialization(Box<dyn std::error::Error + Sync + Send>),

    #[error("Failed to initialize behaviour: {0}")]
    BehaviourInitialization(Box<dyn std::error::Error + Sync + Send>),

=======
/// Errors from libp2p
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// Transport error, multiple reasons and OS-dependent.
    #[error("Failed to listen: {0}")]
    Listen(#[from] TransportError<io::Error>),

    /// The event channel somehow is closed.
    #[error("Events channel closed: {0}")]
    EventsChannelClosed(Box<dyn std::error::Error + Sync + Send>),

    /// Transport error, multiple reasons and OS-dependent.
    ///
    /// Can happen on really bad connections.
    #[error("Failed to initialize transport: {0}")]
    TransportInitialization(Box<dyn std::error::Error + Sync + Send>),

    /// Something is wrong on code level. Maybe ours problem, maybe user's misconfiguration or
    /// whatever.
    #[error("Failed to initialize behaviour: {0}")]
    BehaviourInitialization(Box<dyn std::error::Error + Sync + Send>),

    /// Failed to send response.
>>>>>>> dev-v2
    #[error("Failed to send response: {0}")]
    ResponseError(String),
}
