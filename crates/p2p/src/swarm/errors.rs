//! Swarm errors.

use std::io;

use libp2p::TransportError;
use thiserror::Error;

/// P2P result type.
pub type P2PResult<T> = Result<T, Error>;

/// Swarm errors.
#[derive(Debug, Error)]
pub enum Error {
    /// Validation errors.
    #[error("Validation error {0}")]
    Validation(#[from] ValidationError),

    /// Protocol errors.
    #[error("Protocol error {0}")]
    Protocol(#[from] ProtocolError),
}

/// Validation errors.
#[derive(Debug, Error)]
pub enum ValidationError {
    /// invalid signature
    #[error("Invalid signature")]
    InvalidSignature,
    /// somehow bypassed allowed peers behaviour ?
    #[error("Not in signers allowlist")]
    NotInSignersAllowlist,
}

/// Errors from libp2p
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// failed to listen? is a port already taken?
    #[error("Failed to listen: {0}")]
    Listen(#[from] TransportError<io::Error>),

    /// something is insanely wrong
    #[error("Events channel closed: {0}")]
    EventsChannelClosed(Box<dyn std::error::Error + Sync + Send>),

    /// protocol failed? tcp or whatever protocol with need to wait for acknowledgement failed, or
    /// internet connection is really bad.
    #[error("Failed to initialize transport: {0}")]
    TransportInitialization(Box<dyn std::error::Error + Sync + Send>),

    /// Something is wrong on code level. Maybe ours problem, maybe users misconfiguration or
    /// whatever.
    #[error("Failed to initialize behaviour: {0}")]
    BehaviourInitialization(Box<dyn std::error::Error + Sync + Send>),

    /// Failed to send response
    #[error("Failed to send response: {0}")]
    ResponseError(String),
}
