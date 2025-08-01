//! Our DHT record type.

use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::{Multiaddr, PeerId, identity::PublicKey};
use serde::{Deserialize, Serialize};
use tracing::{trace, warn};

use crate::swarm::{
    serializing::{
        pubkey_serialization::pubkey_serializer,
    }, signed_data::SignedData,
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

pub(crate) fn deserialize_and_validate_dht_record(data: &[u8]) -> Option<RecordData> {
    let str = match String::from_utf8(data.to_owned()) {
        Ok(s) => s,
        Err(e) => {
            warn!(%e, record = %String::from_utf8_lossy(data),
                "Tried to get record from DHT, but it seems to have a string for multiaddress to be invalid UTF-8.");
            return None;
        }
    };

    trace!(%str, "We got a record.");

    let signed_data: SignedData = match serde_json::from_str(&str) {
        Ok(data) => data,
        Err(e) => {
            warn!(%e, "Tried to get record from DHT, but we failed deserializing its signature.");
            return None;
        }
    };

    let str_record = match String::from_utf8(signed_data.raw_data.clone()) {
        Ok(s) => s,
        Err(e) => {
            warn!(%e, record = %String::from_utf8_lossy(&signed_data.raw_data),
            "Tried to get record from DHT, but it seems to have a string for multiaddress to be invalid UTF-8."
            );
            return None;
        }
    };

    let record: RecordData = match serde_json::from_str(&str_record) {
        Ok(data) => data,
        Err(e) => {
            warn!(%e, "Tried to get record from DHT, but we failed deserializing its data.");
            return None;
        }
    };

    if !record
        .app_public_key
        .verify(&signed_data.raw_data, &signed_data.signature)
    {
        warn!("Tried to get record from DHT, but it has bad signature.");
        return None;
    }

    trace!("Returning a record");
    Some(record)
}
