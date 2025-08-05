//! Our DHT record type.

use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::{Multiaddr, identity::PublicKey};
use serde::{Deserialize, Serialize};

use super::signed::{HasAppPublicKey, SignedMessage};
use crate::swarm::serializing::pubkey_serialization::pubkey_serializer;

/// DHT version enum.
#[repr(u8)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DHTProtocolVersion {
    /// first version.
    V1,
}

/// Record structure for DHT.
/// (De)serializable via serde.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordData {
    /// Protocol version.
    pub version: DHTProtocolVersion,

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
            version: DHTProtocolVersion::V1,
            app_public_key,
            date: timestamp,
            multiaddresses,
        }
    }
}

impl HasAppPublicKey for RecordData {
    fn app_public_key(&self) -> &PublicKey {
        &self.app_public_key
    }
}

/// Signed DHT record.
pub type SignedRecord = SignedMessage<RecordData>;
