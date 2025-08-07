//! Message types for P2P protocol communication.

#[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "byos")]
use libp2p::PeerId;
#[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
use libp2p::identity::PublicKey;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
use super::errors::{SetupError, SignedMessageError};
#[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
use crate::signer::ApplicationSigner;

#[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
pub(super) mod pubkey_serializer {
    use serde::{self, Deserializer, Serializer, de};

    use super::PublicKey;

    pub(super) fn serialize<S>(data: &PublicKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&data.encode_protobuf())
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<PublicKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        PublicKey::try_decode_protobuf(&bytes)
            .map_err(|_| de::Error::custom("Failed to deserialize pubkey"))
    }
}

pub(super) mod signature_serializer {
    use serde::{self, Deserializer, Serializer, de};

    pub(super) fn serialize<S>(data: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(data)
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        bytes
            .try_into()
            .map_err(|_| de::Error::custom("Signature must be exactly 64 bytes"))
    }
}

/// Protocol version for all messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub(crate) enum ProtocolVersion {
    /// Version 1 of the protocol.
    V1 = 1,

    /// Version 2 of the protocol.
    #[default]
    V2 = 2,
}

impl From<ProtocolVersion> for u8 {
    fn from(version: ProtocolVersion) -> Self {
        version as u8
    }
}

impl TryFrom<u8> for ProtocolVersion {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ProtocolVersion::V1),
            2 => Ok(ProtocolVersion::V2),
            _ => Err("Invalid protocol version"),
        }
    }
}

/// Protocol identifiers for different message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub(crate) enum ProtocolId {
    /// Setup protocol for peer handshake.
    Setup = 1,

    /// Gossipsub protocol for pub/sub messaging.
    #[cfg(feature = "gossipsub")]
    Gossip = 2,

    /// Request-response protocol for direct communication.
    #[cfg(feature = "request-response")]
    RequestResponse = 3,
}

impl From<ProtocolId> for u8 {
    fn from(protocol: ProtocolId) -> Self {
        protocol as u8
    }
}

impl TryFrom<u8> for ProtocolId {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ProtocolId::Setup),

            #[cfg(feature = "gossipsub")]
            2 => Ok(ProtocolId::Gossip),

            #[cfg(feature = "request-response")]
            3 => Ok(ProtocolId::RequestResponse),

            _ => Err("Invalid protocol ID"),
        }
    }
}

/// Wrapper for signed messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedMessage {
    /// The serialized message content.
    pub message: Vec<u8>,

    /// The signature of the message.
    #[serde(with = "signature_serializer")]
    pub signature: [u8; 64],
}

impl SignedMessage {
    /// Creates a new signed message with the given signer.
    #[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
    pub(crate) fn new<T>(
        message: T,
        signer: &dyn ApplicationSigner,
        app_public_key: PublicKey,
    ) -> Result<Self, SignedMessageError>
    where
        T: Serialize,
    {
        let message_bytes =
            serde_json::to_vec(&message).map_err(|e| SignedMessageError::JsonCodec(e.into()))?;
        let signature = signer
            .sign(&message_bytes, app_public_key)
            .map_err(SignedMessageError::SignedMessageCreation)?;

        Ok(Self {
            message: message_bytes,
            signature,
        })
    }

    /// Verifies the signature of this message.
    #[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
    pub(crate) fn verify_signature(&self, app_public_key: &PublicKey) -> Result<bool, SetupError> {
        Ok(app_public_key.verify(&self.message, &self.signature))
    }

    /// Deserializes the inner message.
    #[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
    pub(crate) fn deserialize_message<T>(&self) -> Result<T, SignedMessageError>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        serde_json::from_slice(&self.message)
            .map_err(|e| SignedMessageError::DeserializationFailed(e.into()))
    }

    /// Deserializes a signed message from JSON bytes.
    #[cfg(any(feature = "gossipsub", feature = "request-response"))]
    pub(crate) fn from_json_bytes(data: &[u8]) -> Result<Self, SignedMessageError> {
        serde_json::from_slice(data)
            .map_err(|e| SignedMessageError::DeserializationFailed(e.into()))
    }
}

impl SignedMessage {
    /// Creates a new signed setup message with the given signer.
    #[cfg(feature = "byos")]
    pub(crate) fn new_signed_setup(
        app_public_key: PublicKey,
        local_transport_id: PeerId,
        remote_transport_id: PeerId,
        signer: &dyn ApplicationSigner,
    ) -> Result<Self, SignedMessageError> {
        let setup_message = SetupMessage::new(
            app_public_key.clone(),
            local_transport_id,
            remote_transport_id,
        );

        SignedMessage::new(setup_message, signer, app_public_key)
    }

    /// Creates a new signed gossipsub message with the given signer (convenience method).
    #[cfg(feature = "gossipsub")]
    pub(crate) fn new_signed_gossip(
        app_public_key: PublicKey,
        message: Vec<u8>,
        signer: &dyn ApplicationSigner,
    ) -> Result<Self, SignedMessageError> {
        let gossip_message = GossipMessage::new(app_public_key.clone(), message);

        SignedMessage::new(gossip_message, signer, app_public_key)
    }

    /// Creates a new signed request message with the given signer (convenience method).
    #[cfg(feature = "request-response")]
    pub(crate) fn new_signed_request(
        app_public_key: PublicKey,
        message: Vec<u8>,
        signer: &dyn ApplicationSigner,
    ) -> Result<Self, SignedMessageError> {
        let request_message = RequestMessage::new(app_public_key.clone(), message);

        SignedMessage::new(request_message, signer, app_public_key)
    }

    /// Creates a new signed response message with the given signer (convenience method).
    #[cfg(feature = "request-response")]
    pub(crate) fn new_signed_response(
        app_public_key: PublicKey,
        message: Vec<u8>,
        signer: &dyn ApplicationSigner,
    ) -> Result<Self, SignedMessageError> {
        let response_message = ResponseMessage::new(app_public_key.clone(), message);

        SignedMessage::new(response_message, signer, app_public_key)
    }
}

/// Setup message structure for the handshake protocol.
/// Now serialized/deserialized using JSON instead of custom binary format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg(feature = "byos")]
pub(crate) struct SetupMessage {
    /// Protocol version.
    pub version: ProtocolVersion,
    /// Protocol identifier (setup).
    pub protocol: ProtocolId,
    /// The application public key (Ed25519) - stored as bytes for serialization.
    #[serde(with = "pubkey_serializer")]
    pub app_public_key: PublicKey,
    /// Local transport ID (PeerId) - our transport ID.
    pub local_transport_id: PeerId,
    /// Remote transport ID (PeerId) - transport ID of the destination peer.
    pub remote_transport_id: PeerId,
    /// Timestamp of message creation.
    pub date: u64,
}

#[cfg(feature = "byos")]
impl SetupMessage {
    /// Creates a new setup message with the given parameters.
    pub(crate) fn new(
        app_public_key: PublicKey,
        local_transport_id: PeerId,
        remote_transport_id: PeerId,
    ) -> Self {
        let timestamp = get_timestamp();

        Self {
            version: ProtocolVersion::default(),
            protocol: ProtocolId::Setup,
            app_public_key,
            local_transport_id,
            remote_transport_id,
            date: timestamp,
        }
    }
}

/// Gossipsub message structure for the gossipsub protocol.
/// Serialized/deserialized using JSON format.
#[cfg(feature = "gossipsub")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct GossipMessage {
    /// Protocol version.
    pub version: ProtocolVersion,
    /// Protocol identifier (gossipsub).
    pub protocol: ProtocolId,
    /// Gossipsub message data
    pub message: Vec<u8>,
    /// The application public key (Ed25519).
    #[serde(with = "pubkey_serializer")]
    pub app_public_key: PublicKey,
    /// Timestamp of message creation.
    pub date: u64,
}

#[cfg(feature = "gossipsub")]
impl GossipMessage {
    /// Creates a new gossipsub message with the given parameters.
    pub(crate) fn new(app_public_key: PublicKey, message: Vec<u8>) -> Self {
        let timestamp = get_timestamp();

        Self {
            version: ProtocolVersion::default(),
            protocol: ProtocolId::Gossip,
            message,
            app_public_key,
            date: timestamp,
        }
    }
}

/// Request message structure for the request-response protocol.
/// Serialized/deserialized using JSON format.
#[cfg(feature = "request-response")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RequestMessage {
    /// Protocol version.
    pub version: ProtocolVersion,
    /// Protocol identifier (request-response).
    pub protocol: ProtocolId,
    /// Request-response message data
    pub message: Vec<u8>,
    /// The application public key (Ed25519).
    #[serde(with = "pubkey_serializer")]
    pub app_public_key: PublicKey,
    /// Timestamp of message creation.
    pub date: u64,
}

#[cfg(feature = "request-response")]
impl RequestMessage {
    /// Creates a new request-response message with the given parameters.
    pub(crate) fn new(app_public_key: PublicKey, message: Vec<u8>) -> Self {
        let timestamp = get_timestamp();

        Self {
            version: ProtocolVersion::default(),
            protocol: ProtocolId::RequestResponse,
            message,
            app_public_key,
            date: timestamp,
        }
    }
}

/// Response message structure for the request-response protocol.
/// Serialized/deserialized using JSON format.
#[cfg(feature = "request-response")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ResponseMessage {
    /// Protocol version.
    pub version: ProtocolVersion,
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
            version: ProtocolVersion::default(),
            protocol: ProtocolId::RequestResponse,
            message,
            app_public_key,
            date: timestamp,
        }
    }
}

#[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
