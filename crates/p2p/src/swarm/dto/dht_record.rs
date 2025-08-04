//! Our DHT record type.

use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::{Multiaddr, PeerId, identity::PublicKey};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    signer::ApplicationSigner,
    swarm::{
        dto::signed_data::SignedData,
        serializing::{
            pubkey_serialization::pubkey_serializer, signature_serialization::signature_serializer,
        },
    },
};

/// Protocol version for DHT records.
pub(crate) const DHT_PROTOCOL_VERSION: u8 = 0;

/// Protocol identifier for DHT records.
pub(crate) const DHT_PROTOCOL_ID: u8 = 0;

/// Record structure for DHT.
/// (De)serializable via serde.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordData {
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

/// Signed DHT record.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SignedRecord {
    /// underlying record
    pub record: RecordData,
    /// Signature of record by application keypair,
    /// that can be verified by application public key from record field.
    #[serde(with = "signature_serializer")]
    pub signature: [u8; 64],
}

impl SignedRecord {
    fn new<S: ApplicationSigner>(
        record: RecordData,
        signer: S,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let signature = signer.sign(&serde_json::to_vec(&record)?, record.app_public_key.clone())?;
        Ok(Self { record, signature })
    }
}

impl<'de> Deserialize<'de> for SignedRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let signed_data: SignedData = serde::Deserialize::deserialize(deserializer)?;

        let record: RecordData =
            serde_json::from_slice(&signed_data.raw_data).map_err(serde::de::Error::custom)?;

        if !record
            .app_public_key
            .verify(&signed_data.raw_data, &signed_data.signature)
        {
            return Err(de::Error::custom("Signature verification failed"));
        }

        Ok(SignedRecord {
            record,
            signature: signed_data.signature,
        })
    }
}
