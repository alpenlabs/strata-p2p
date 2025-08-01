//! Message types for P2P protocol communication.

use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::{PeerId, identity::PublicKey};
use serde::{Deserialize, Serialize};

use crate::swarm::serializing::pubkey_serialization::pubkey_serializer;

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
