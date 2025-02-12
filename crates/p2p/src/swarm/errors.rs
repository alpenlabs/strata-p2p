//! Swarm errors.

use std::io;

use libp2p::TransportError;
use strata_p2p_db::RepositoryError;
use thiserror::Error;

/// P2P result type.
pub type P2PResult<T> = Result<T, Error>;

/// Swarm errors.
#[derive(Debug, Error)]
pub enum Error {
    /// Database errors.
    #[error("Database error")]
    Repository(#[from] RepositoryError),

    // Validation errors.
    #[error("Validation error")]
    Validation(#[from] ValidationError),

    /// Protocol errors.
    #[error("Protocol error")]
    Protocol(#[from] ProtocolError),
}

/// Validation errors.
#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Not in signers allowlist")]
    NotInSignersAllowlist,
}

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
}
