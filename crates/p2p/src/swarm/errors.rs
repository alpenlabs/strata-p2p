//! Swarm errors.

use std::{error, io};

use libp2p::TransportError;
use thiserror::Error;

/// P2P result type.
pub type P2PResult<T> = Result<T, SwarmError>;

/// Errors that can happen during a setup handshake.
#[derive(Debug, Error)]
pub enum SetupError {
    /// Indicates that signature verification failed.
    ///
    /// This event is fired when the signature verification fails for a peer's
    /// handshake message, indicating the connection should be dropped.
    #[error("Signature verification failed")]
    SignatureVerificationFailed,

    /// In received message application public key is invalid.
    #[error("Application public key invalid: {0}")]
    AppPublicKeyInvalid(Box<dyn error::Error + Send + Sync>),

    /// Error during sending to remote peer.
    #[error("Outbound error: {0}")]
    OutboundError(Box<dyn error::Error + Send + Sync>),

    /// Error during receiving from remote peer.
    #[error("Inbound error: {0}")]
    InboundError(Box<dyn error::Error + Send + Sync>),
}

/// Swarm errors.
#[derive(Debug, Error)]
pub enum SwarmError {
    /// Protocol errors.
    #[error("Protocol error {0}")]
    Protocol(#[from] ProtocolError),
}

/// Errors that can occur during the setup upgrade process.
#[derive(Debug, Error)]
pub enum SetupUpgradeError {
    /// Failed to create a signed message.
    #[error("Failed to create signed message: {0}")]
    SignedMessageCreation(Box<dyn error::Error + Send + Sync>),

    /// Codec error (Flexbuffers serialization/deserialization).
    #[error("Codec error: {0}")]
    Codec(#[from] io::Error),

    /// Stream was closed unexpectedly.
    #[error("Stream closed unexpectedly")]
    UnexpectedStreamClose,
}

/// Generic errors that can occur during signed message operations.
#[derive(Debug, Error)]
pub enum SignedMessageError {
    /// Codec error (Flexbuffers serialization/deserialization).
    #[error("Codec error: {0}")]
    Codec(#[from] io::Error),

    /// Signature verification failed.
    #[error("Signature verification failed")]
    SignatureVerificationFailed,

    /// Failed to create a signed message.
    #[error("Failed to create signed message: {0}")]
    SignedMessageCreation(Box<dyn error::Error + Send + Sync>),
}

/// Protocol errors.
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
