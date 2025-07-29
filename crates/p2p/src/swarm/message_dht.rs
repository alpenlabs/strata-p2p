use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::{Multiaddr, PeerId, identity::PublicKey};
use serde::{Deserialize, Serialize};

use crate::swarm::{
    errors::DHTError,
    message::{
        pubkey_serialization::pubkey_serializer, signature_serialization::signature_serializer,
    },
};

/// Protocol version for DHT records.
pub(crate) const DHT_PROTOCOL_VERSION: u8 = 0;

/// Protocol identifier for DHT records.
pub(crate) const DHT_PROTOCOL_ID: u8 = 0;

/// Wrapper for signed record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SignedRecordData {
    /// The serialized record data.
    pub record: Vec<u8>,
    /// The signature of the record.
    #[serde(with = "signature_serializer")]
    pub signature: [u8; 64],
}

/// Record structure for DHT.
/// (De)serializable via serde.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RecordData {
    /// Protocol version.
    pub version: u8,
    /// Protocol identifier.
    pub protocol: u8,
    /// The application public key (Ed25519) - stored as bytes for serialization.
    #[serde(with = "pubkey_serializer")]
    pub app_public_key: PublicKey,
    /// Transport ID (PeerId).
    pub transport_id: PeerId,
    /// Multiaddresses.
    pub multiaddresses: Vec<Multiaddr>,
    /// Timestamp of message creation.
    pub date: u64,
}

impl RecordData {
    pub(crate) fn new(
        app_public_key: PublicKey,
        transport_id: PeerId,
        multiaddresses: Vec<Multiaddr>,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            version: DHT_PROTOCOL_VERSION,
            protocol: DHT_PROTOCOL_ID,
            app_public_key,
            transport_id,
            date: timestamp,
            multiaddresses,
        }
    }
}

impl SignedRecordData {
    /// Creates a new signed message with the given signer.
    pub(crate) fn new<T, S>(
        something_serializable: T,
        signer: &S,
        app_public_key: PublicKey,
    ) -> Result<Self, DHTError>
    where
        T: Serialize,
        S: crate::signer::ApplicationSigner,
    {
        let message_bytes = serde_json::to_vec(&something_serializable)
            .map_err(|e| DHTError::JsonCodec(e.into()))?;
        let signature = signer
            .sign(&message_bytes, app_public_key)
            .map_err(DHTError::SignedMessageCreation)?;

        Ok(Self {
            record: message_bytes,
            signature,
        })
    }

    /// Verifies the signature of this record.
    pub(crate) fn verify_signature(&self, app_public_key: &PublicKey) -> bool {
        app_public_key.verify(&self.record, &self.signature)
    }

    /// Deserializes a signed record from JSON bytes.
    #[expect(dead_code)]
    pub(crate) fn from_json_bytes(data: &[u8]) -> Result<Self, DHTError> {
        serde_json::from_slice(data).map_err(|e| DHTError::DeserializationFailed(e.into()))
    }
}

impl SignedRecordData {
    /// Creates a new signed record with the given signer.
    pub(crate) fn new_signed_record<S: crate::signer::ApplicationSigner>(
        app_public_key: PublicKey,
        transport_id: PeerId,
        multiadresses: Vec<Multiaddr>,
        signer: &S,
    ) -> Result<Self, DHTError> {
        let setup_message = RecordData::new(app_public_key.clone(), transport_id, multiadresses);

        SignedRecordData::new(setup_message, signer, app_public_key)
    }
}
