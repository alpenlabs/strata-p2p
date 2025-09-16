//! Our DHT record type.

use libp2p::{Multiaddr, identity::PublicKey};
use serde::{Deserialize, Serialize};

use super::signed::{HasPublicKey, SignedMessage};
use crate::swarm::message::{get_timestamp, serde::pubkey_serializer};

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

    /// The public key (Ed25519).
    #[serde(with = "pubkey_serializer")]
    pub public_key: PublicKey,

    /// Multiaddresses.
    pub multiaddresses: Vec<Multiaddr>,

    /// Timestamp of message creation.
    pub date: u64,
}

impl RecordData {
    pub(crate) fn new(public_key: PublicKey, multiaddresses: Vec<Multiaddr>) -> Self {
        let timestamp = get_timestamp();

        Self {
            version: DHTProtocolVersion::V1,
            public_key,
            date: timestamp,
            multiaddresses,
        }
    }
}

impl HasPublicKey for RecordData {
    fn public_key(&self) -> &PublicKey {
        &self.public_key
    }
}

/// Signed DHT record.
pub type SignedRecord = SignedMessage<RecordData>;
