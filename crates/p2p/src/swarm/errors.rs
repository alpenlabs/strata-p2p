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
    /// Invalid signature
    #[error("Invalid signature")]
    InvalidSignature,
    /// Not whitelisted
    #[error("Not in signers allowlist")]
    NotInSignersAllowlist,
}

/// Error at network level
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// Failed to setup listening at a port with a protocol
    #[error("Failed to listen: {0}")]
    Listen(#[from] TransportError<io::Error>),

    /// Somehow event channel appeared closed
    #[error("Events channel closed: {0}")]
    EventsChannelClosed(Box<dyn std::error::Error + Sync + Send>),

    /// Transport initialization failed
    #[error("Failed to initialize transport: {0}")]
    TransportInitialization(Box<dyn std::error::Error + Sync + Send>),

    /// Behaviour initialization failed
    #[error("Failed to initialize behaviour: {0}")]
    BehaviourInitialization(Box<dyn std::error::Error + Sync + Send>),

    /// Failed sending back
    #[error("Failed to send response: {0}")]
    ResponseError(String),
}
