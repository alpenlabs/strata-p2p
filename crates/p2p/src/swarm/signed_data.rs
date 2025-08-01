use serde::{Deserialize, Serialize};

use libp2p::{PeerId, identity::PublicKey};
use super::errors::{SetupError, SetupUpgradeError};
use crate::swarm::serializing::signature_serialization::signature_serializer;
use crate::swarm::message::SetupMessage;

/// Wrapper for signed messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedData {
    /// The serialized message content.
    pub message: Vec<u8>,
    /// The signature of the message.
    #[serde(with = "signature_serializer")]
    pub signature: [u8; 64],
}

impl SignedData {
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
        let message_bytes =
            serde_json::to_vec(&message).map_err(|e| SetupUpgradeError::JsonCodec(e.into()))?;
        let signature = signer
            .sign(&message_bytes, app_public_key)
            .map_err(SetupUpgradeError::SignedMessageCreation)?;

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
        serde_json::from_slice(data).map_err(|e| SetupError::DeserializationFailed(e.into()))
    }
}

impl SignedData {
    /// Creates a new signed setup message with the given signer.
    pub(crate) fn new_signed_setup<S: crate::signer::ApplicationSigner>(
        app_public_key: PublicKey,
        local_transport_id: PeerId,
        remote_transport_id: PeerId,
        signer: &S,
    ) -> Result<Self, SetupUpgradeError> {
        let setup_message = SetupMessage::new(
            app_public_key.clone(),
            local_transport_id,
            remote_transport_id,
        );

        SignedData::new(setup_message, signer, app_public_key)
    }
}
