//! Our DHT record type.

use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::{Multiaddr, identity::PublicKey};
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    signer::ApplicationSigner,
    swarm::serializing::{
        pubkey_serialization::pubkey_serializer, signature_serialization::signature_serializer,
    },
};

/// Protocol version for DHT records.
pub(crate) const DHT_PROTOCOL_VERSION: u8 = 0;

/// Record structure for DHT.
/// (De)serializable via serde.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordData {
    /// Protocol version.
    pub version: u8,
    /// The application public key (Ed25519).
    #[serde(with = "pubkey_serializer")]
    pub app_public_key: PublicKey,
    /// Multiaddresses.
    pub multiaddresses: Vec<Multiaddr>,
    /// Timestamp of message creation.
    pub date: u64,
}

impl RecordData {
    pub(crate) fn new(app_public_key: PublicKey, multiaddresses: Vec<Multiaddr>) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            version: DHT_PROTOCOL_VERSION,
            app_public_key,
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
    /// Create new [`SignedRecord`] from [`RecordData`] and sign via signer that implements
    /// [`ApplicationSigner`]
    pub fn new<S: ApplicationSigner>(
        record: RecordData,
        signer: &S,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let signature =
            signer.sign(&serde_json::to_vec(&record)?, record.app_public_key.clone())?;
        Ok(Self { record, signature })
    }
}

impl<'de> Deserialize<'de> for SignedRecord {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        struct SignedRecordHelper {
            pub record: RecordData,
            #[serde(with = "signature_serializer")]
            pub signature: [u8; 64],
        }

        let signed_record: SignedRecordHelper = serde::Deserialize::deserialize(deserializer)?;

        if !signed_record.record.app_public_key.verify(
            &serde_json::to_vec(&signed_record.record).map_err(|_| {
                serde::de::Error::custom(
                    "Failed to serialize deserialized, for signature verification",
                )
            })?,
            &signed_record.signature,
        ) {
            return Err(de::Error::custom("Signature verification failed"));
        }

        Ok(SignedRecord {
            record: signed_record.record,
            signature: signed_record.signature,
        })
    }
}
