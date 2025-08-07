//! Message types for Request-response request and response messages.

#![cfg(feature = "request-response")]

use libp2p::identity::PublicKey;
use serde::{Deserialize, Serialize};

use crate::swarm::message::{
    ProtocolId, get_timestamp,
    serde::pubkey_serializer,
    signed::{HasPublicKey, SignedMessage},
};

/// Request-response *request* protocol version enum.
#[repr(u8)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RRRequestProtocolVersion {
    /// first version.
    V1,
    /// second (current last) version.
    V2,
}

/// Request-response *response* protocol version enum.
#[repr(u8)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RRResponseProtocolVersion {
    /// first version.
    V1,
    /// second (current last) version.
    V2,
}

/// Request message structure for the request-response protocol.
/// Serialized/deserialized using JSON format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestMessage {
    /// Protocol version.
    pub version: RRRequestProtocolVersion,
    /// Protocol identifier (request-response).
    pub protocol: ProtocolId,
    /// Request-response message data
    pub message: Vec<u8>,
    /// The public key (Ed25519). Application public key if BYOS is enabled, otherwise transport
    /// public key.
    #[serde(with = "pubkey_serializer")]
    pub public_key: PublicKey,
    /// Timestamp of message creation.
    pub date: u64,
}

#[cfg(feature = "request-response")]
impl RequestMessage {
    /// Creates a new request-response message with the given parameters.
    pub fn new(app_public_key: PublicKey, message: Vec<u8>) -> Self {
        let timestamp = get_timestamp();

        Self {
            version: RRRequestProtocolVersion::V2,
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
#[cfg(feature = "request-response")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseMessage {
    /// Protocol version.
    pub version: RRResponseProtocolVersion,
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

#[cfg(feature = "request-response")]
impl ResponseMessage {
    /// Creates a new response message with the given parameters.
    pub(crate) fn new(app_public_key: PublicKey, message: Vec<u8>) -> Self {
        let timestamp = get_timestamp();

        Self {
            version: RRResponseProtocolVersion::V2,
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
