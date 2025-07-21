//! Swarm errors.

use std::{error, io};

use libp2p::TransportError;
use thiserror::Error;

/// P2P result type.
pub type P2PResult<T> = Result<T, Error>;

/// Errors for validatation inside of validate.
#[derive(Debug, Clone)]
pub enum SetupMessageValidationError {
    /// Version mismatch: version of message is not supported or is incorrect.
    VersionMismatch,
    /// Protocol mismatch: protocol of message is not supported or is incorrect.
    ProtocolMismatch,
    /// Application public key in message is somehow empty.
    AppPublicKeyEmpty,
    /// Local (our) transport id ( [`PeerId`] ) is missing.
    LocalTransportIdEmpty,
    /// Remote (someone's) transport id ( [`PeerId`] ) is missing.
    RemoteTransportIdEmpty,
}

impl std::fmt::Display for SetupMessageValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SetupMessageValidationError::VersionMismatch => write!(f, "Invalid protocol version"),
            SetupMessageValidationError::ProtocolMismatch => write!(f, "Invalid protocol ID"),
            SetupMessageValidationError::AppPublicKeyEmpty => {
                write!(f, "Application public key is empty")
            }
            SetupMessageValidationError::LocalTransportIdEmpty => {
                write!(f, "Local transport ID is empty")
            }
            SetupMessageValidationError::RemoteTransportIdEmpty => {
                write!(f, "Remote transport ID is empty")
            }
        }
    }
}

impl std::error::Error for SetupMessageValidationError {}

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
    /// The signature is invalid.
    #[error("Invalid signature")]
    InvalidSignature,
    /// The message signer is not in the signer's allowlist.
    #[error("Not in signers allowlist")]
    NotInSignersAllowlist,
}

/// Errors from libp2p
#[derive(Debug, Error)]
pub enum ProtocolError {
    /// Transport error, multiple reasons and OS-dependent.
    #[error("Failed to listen: {0}")]
    Listen(#[from] TransportError<io::Error>),

    /// The gossip channel somehow is closed.
    #[error("Gossip channel closed: {0}")]
    GossipEventsChannelClosed(Box<dyn error::Error + Sync + Send>),

    /// The request response event channel somehow is closed.
    #[cfg(feature = "request-response")]
    #[error("Request response channel closed: {0}")]
    ReqRespEventChannelClosed(Box<dyn error::Error + Sync + Send>),

    /// Transport error, multiple reasons and OS-dependent.
    ///
    /// Can happen on really bad connections.
    #[error("Failed to initialize transport: {0}")]
    TransportInitialization(Box<dyn error::Error + Sync + Send>),

    /// Something is wrong on code level. Maybe ours problem, maybe user's misconfiguration or
    /// whatever.
    #[error("Failed to initialize behaviour: {0}")]
    BehaviourInitialization(Box<dyn error::Error + Sync + Send>),

    /// Failed to send response.
    #[error("Failed to send response: {0}")]
    ResponseError(String),
}
