//! Message types for P2P protocol communication.

use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::{PeerId, identity::PublicKey};
use serde::{Deserialize, Serialize};

use super::errors::{SetupError, SetupUpgradeError};

pub(super) mod opaque_serializer {
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
        PublicKey::try_decode_protobuf(&bytes).map_err(de::Error::custom)
    }
}

/// Protocol version for all messages.
pub(crate) const PROTOCOL_VERSION: u8 = 2;

/// Protocol identifier for setup messages.
pub(crate) const SETUP_PROTOCOL_ID: u8 = 1;

/// Wrapper for signed messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedMessage {
    /// The serialized message content.
    pub message: Vec<u8>,
    /// The signature of the message.
    pub signature: Vec<u8>,
}

impl SignedMessage {
    /// Creates a new signed message with the given signer.
    pub(crate) fn new<T, S>(
        message: T,
        signer: &S,
        app_public_key: PublicKey,
    ) -> Result<Self, SetupUpgradeError>
    where
        T: Serialize,
        S: crate::signer::ApplicationSigner,
    {
        let message_bytes = serde_json::to_vec(&message)
            .map_err(|e| SetupUpgradeError::JsonCodec(e.into()))?;
        let signature = signer.sign(&message_bytes, app_public_key)
            .map_err(SetupUpgradeError::SignedMessageCreation)?;

        Ok(Self {
            message: message_bytes,
            signature,
        })
    }

    /// Verifies the signature of this message.
    pub(crate) fn verify_signature(
        &self,
        app_public_key: &PublicKey,
    ) -> Result<bool, SetupError> {
        Ok(app_public_key.verify(&self.message, &self.signature))
    }

    /// Deserializes the inner message.
    pub(crate) fn deserialize_message<T>(&self) -> Result<T, SetupError>
    where
        T: for<'de> serde::Deserialize<'de>,
    {
        serde_json::from_slice(&self.message)
            .map_err(|e| SetupError::DeserializationFailed(e.into()))
    }

    /// Deserializes a signed message from JSON bytes.
    #[expect(dead_code)]
    pub(crate) fn from_json_bytes(data: &[u8]) -> Result<Self, SetupError> {
        serde_json::from_slice(data)
            .map_err(|e| SetupError::DeserializationFailed(e.into()))
    }
}

impl SignedMessage {
    /// Creates a new signed setup message with the given signer.
    pub(crate) fn new_signed_setup<S: crate::signer::ApplicationSigner>(
        app_public_key: PublicKey,
        local_transport_id: PeerId,
        remote_transport_id: PeerId,
        signer: &S,
    ) -> Result<Self, SetupUpgradeError> {
        let setup_message =
            SetupMessage::new(app_public_key.clone(), local_transport_id, remote_transport_id);

        SignedMessage::new(setup_message, signer, app_public_key)
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
    #[serde(with = "opaque_serializer")]
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
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

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
