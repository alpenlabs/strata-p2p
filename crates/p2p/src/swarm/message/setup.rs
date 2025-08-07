//! Message types for P2P protocol communication inside of Setup behaviour's upgrade.
//! It used for exchanging public key of the remote peer if `byos` is enabled.

use libp2p::{PeerId, identity::PublicKey};
use serde::{Deserialize, Serialize};

use super::signed::{HasPublicKey, SignedMessage};
use crate::swarm::message::{get_timestamp, serde::pubkey_serializer};

/// Setup protocol version enum.
#[repr(u8)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SetupProtocolVersion {
    /// First version.
    #[default]
    V1,
}

/// Setup message structure for the handshake protocol.
/// Now serialized/deserialized using JSON instead of custom binary format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetupMessage {
    /// Protocol version.
    pub version: SetupProtocolVersion,
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
        let timestamp = get_timestamp();

        Self {
            version: SetupProtocolVersion::default(),
            app_public_key,
            local_transport_id,
            remote_transport_id,
            date: timestamp,
        }
    }
}

impl HasPublicKey for SetupMessage {
    fn public_key(&self) -> &PublicKey {
        &self.app_public_key
    }
}

/// Type alias for signed setup message.
pub type SignedSetupMessage = SignedMessage<SetupMessage>;
