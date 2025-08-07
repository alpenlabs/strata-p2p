//! Message types for Request-response request and response messages.

use libp2p::identity::PublicKey;
use serde::{Deserialize, Serialize};

use crate::swarm::message::{
    ProtocolId, get_timestamp,
    serde::pubkey_serializer,
    signed::{HasPublicKey, SignedMessage},
};

/// Request-response protocol version enum.
#[repr(u8)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RequestResponseProtocolVersion {
    /// First version.
    V1,
    /// Second version.
    #[default]
    V2,
}

/// Request message structure for the request-response protocol.
/// Serialized/deserialized using JSON format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestMessage {
    /// Protocol version.
    pub version: RequestResponseProtocolVersion,
    /// Protocol identifier (request-response).
    pub protocol: ProtocolId,
    /// Request-response message data
    pub message: Vec<u8>,
    /// The public key (Ed25519). Application public key if `byos` feature is enabled, otherwise
    /// transport
    #[serde(with = "pubkey_serializer")]
    pub public_key: PublicKey,
    /// Timestamp of message creation.
    pub date: u64,
}

impl RequestMessage {
    /// Creates a new request-response message with the given parameters.
    pub fn new(app_public_key: PublicKey, message: Vec<u8>) -> Self {
        let timestamp = get_timestamp();

        Self {
            version: RequestResponseProtocolVersion::V2,
            protocol: ProtocolId::RequestResponse,
            message,
            public_key: app_public_key,
            date: timestamp,
        }
    }
}

impl HasPublicKey for RequestMessage {
    fn public_key(&self) -> &PublicKey {
        &self.public_key
    }
}

/// Response message structure for the request-response protocol.
/// Serialized/deserialized using JSON format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseMessage {
    /// Protocol version.
    pub version: RequestResponseProtocolVersion,
    /// Protocol identifier (request-response).
    pub protocol: ProtocolId,
    /// Response-response message data
    pub message: Vec<u8>,
    /// The application public key (Ed25519).
    #[serde(with = "pubkey_serializer")]
    pub app_public_key: PublicKey,
    /// Timestamp of message creation.
    pub date: u64,
}

impl ResponseMessage {
    /// Creates a new response message with the given parameters.
    pub(crate) fn new(app_public_key: PublicKey, message: Vec<u8>) -> Self {
        let timestamp = get_timestamp();

        Self {
            version: RequestResponseProtocolVersion::default(),
            protocol: ProtocolId::RequestResponse,
            message,
            app_public_key,
            date: timestamp,
        }
    }
}

impl HasPublicKey for ResponseMessage {
    fn public_key(&self) -> &PublicKey {
        &self.app_public_key
    }
}

/// Type alias for signed Request.
pub type SignedRequestMessage = SignedMessage<RequestMessage>;

/// Type alias for signed Response.
pub type SignedResponseMessage = SignedMessage<ResponseMessage>;
