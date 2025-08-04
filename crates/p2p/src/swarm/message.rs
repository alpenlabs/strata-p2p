//! Message types for P2P protocol communication.

use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::{identity::PublicKey, PeerId};
use serde::{Deserialize, Serialize};

use super::errors::{SetupError, SignedMessageError};

pub(super) mod pubkey_serializer {
    use serde::{self, de, Deserializer, Serializer};

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
    use serde::{self, de, Deserializer, Serializer};

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
pub(crate) const PROTOCOL_VERSION: u8 = 2;

/// Protocol identifier for setup messages.
pub(crate) const SETUP_PROTOCOL_ID: u8 = 1;

/// Protocol identifier for gossipsub messages.
#[cfg(feature = "gossipsub")]
pub(crate) const GOSSIP_PROTOCOL_ID: u8 = 2;

/// Protocol identifier for request-response messages.
#[cfg(feature = "request-response")]
pub(crate) const REQ_RESP_PROTOCOL_ID: u8 = 3;

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
    pub(crate) fn new<T, S>(
        message: T,
        signer: &S,
        app_public_key: PublicKey,
    ) -> Result<Self, SignedMessageError>
    where
        T: Serialize,
        S: crate::signer::ApplicationSigner,
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
    pub(crate) fn verify_signature(&self, app_public_key: &PublicKey) -> Result<bool, SetupError> {
        Ok(app_public_key.verify(&self.message, &self.signature))
    }

    /// Deserializes the inner message.
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
    pub(crate) fn new_signed_setup<S: crate::signer::ApplicationSigner>(
        app_public_key: PublicKey,
        local_transport_id: PeerId,
        remote_transport_id: PeerId,
        signer: &S,
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
    pub(crate) fn new_signed_gossip<S: crate::signer::ApplicationSigner>(
        app_public_key: PublicKey,
        message: Vec<u8>,
        signer: &S,
    ) -> Result<Self, SignedMessageError> {
        let gossip_message = GossipMessage::new(app_public_key.clone(), message);

        SignedMessage::new(gossip_message, signer, app_public_key)
    }

    /// Creates a new signed request message with the given signer (convenience method).
    #[cfg(feature = "request-response")]
    pub(crate) fn new_signed_request<S: crate::signer::ApplicationSigner>(
        app_public_key: PublicKey,
        message: Vec<u8>,
        signer: &S,
    ) -> Result<Self, SignedMessageError> {
        let request_message = RequestMessage::new(app_public_key.clone(), message);

        SignedMessage::new(request_message, signer, app_public_key)
    }

    /// Creates a new signed response message with the given signer (convenience method).
    #[cfg(feature = "request-response")]
    pub(crate) fn new_signed_response<S: crate::signer::ApplicationSigner>(
        app_public_key: PublicKey,
        message: Vec<u8>,
        signer: &S,
    ) -> Result<Self, SignedMessageError> {
        let response_message = ResponseMessage::new(app_public_key.clone(), message);

        SignedMessage::new(response_message, signer, app_public_key)
    }
}

/// Setup message structure for the handshake protocol.
/// Now serialized/deserialized using JSON instead of custom binary format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SetupMessage {
    /// Protocol version.
    pub version: u8,
    /// Protocol identifier (setup).
    pub protocol: u8,
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

impl SetupMessage {
    /// Creates a new setup message with the given parameters.
    pub(crate) fn new(
        app_public_key: PublicKey,
        local_transport_id: PeerId,
        remote_transport_id: PeerId,
    ) -> Self {
        let timestamp = get_timestamp();

        Self {
            version: PROTOCOL_VERSION,
            protocol: SETUP_PROTOCOL_ID,
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
    pub version: u8,
    /// Protocol identifier (gossipsub).
    pub protocol: u8,
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
            version: PROTOCOL_VERSION,
            protocol: GOSSIP_PROTOCOL_ID,
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
    pub version: u8,
    /// Protocol identifier (request-response).
    pub protocol: u8,
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
            version: PROTOCOL_VERSION,
            protocol: REQ_RESP_PROTOCOL_ID,
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
    pub version: u8,
    /// Protocol identifier (request-response).
    pub protocol: u8,
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
            version: PROTOCOL_VERSION,
            protocol: REQ_RESP_PROTOCOL_ID,
            message,
            app_public_key,
            date: timestamp,
        }
    }
}

fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
