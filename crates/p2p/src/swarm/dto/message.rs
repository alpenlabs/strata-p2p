//! Message types for P2P protocol communication.

use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::{PeerId, identity::PublicKey};
use serde::{Deserialize, Serialize};

use crate::swarm::serializing::pubkey_serialization::pubkey_serializer;

use super::signed::{HasAppPublicKey, SignedMessage};

/// Protocol version for all messages.
pub(crate) const SETUP_PROTOCOL_VERSION: u8 = 2;

/// Setup message structure for the handshake protocol.
/// Now serialized/deserialized using JSON instead of custom binary format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetupMessage {
    /// Protocol version.
    pub version: u8,
    /// The application public key (Ed25519).
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
    pub fn new(
        app_public_key: PublicKey,
        local_transport_id: PeerId,
        remote_transport_id: PeerId,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            version: SETUP_PROTOCOL_VERSION,
            app_public_key,
            local_transport_id,
            remote_transport_id,
            date: timestamp,
        }
    }
}

impl HasAppPublicKey for SetupMessage {
    fn app_public_key(&self) -> &PublicKey {
        &self.app_public_key
    }
}


/// Type alias for signed setup message.
pub type SignedSetupMessage = SignedMessage<SetupMessage>;
