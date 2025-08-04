//! Message types for P2P protocol communication.

use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::{PeerId, identity::PublicKey};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    signer::ApplicationSigner,
    swarm::serializing::{
        pubkey_serialization::pubkey_serializer, signature_serialization::signature_serializer,
    },
};

/// Protocol version for all messages.
pub(crate) const PROTOCOL_VERSION: u8 = 2;

/// Protocol identifier for setup messages.
pub(crate) const SETUP_PROTOCOL_ID: u8 = 1;

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

/// Signed message.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SignedMessage {
    /// underlying message
    pub(crate) message: SetupMessage,
    /// Signature of message by application keypair,
    /// that can be verified by application public key from message field.
    #[serde(with = "signature_serializer")]
    pub(crate) signature: [u8; 64],
}

impl SignedMessage {
    /// Create new [`SignedMessage`] from [`SetupMessage`] and sign via signer that implements
    /// [`ApplicationSigner`]
    pub(crate) fn new<S: ApplicationSigner>(
        message: SetupMessage,
        signer: &S,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let signature = signer.sign(
            &serde_json::to_vec(&message)?,
            message.app_public_key.clone(),
        )?;
        Ok(Self { message, signature })
    }
}

impl<'de> Deserialize<'de> for SignedMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        struct SignedMessageHelper {
            pub message: SetupMessage,
            #[serde(with = "signature_serializer")]
            pub signature: [u8; 64],
        }

        let signed_message: SignedMessageHelper = serde::Deserialize::deserialize(deserializer)?;

        if !signed_message.message.app_public_key.verify(
            &serde_json::to_vec(&signed_message.message).map_err(|_| {
                serde::de::Error::custom(
                    "Failed to serialize deserialized, for signature verification",
                )
            })?,
            &signed_message.signature,
        ) {
            return Err(de::Error::custom("Signature verification failed"));
        }

        Ok(SignedMessage {
            message: signed_message.message,
            signature: signed_message.signature,
        })
    }
}
