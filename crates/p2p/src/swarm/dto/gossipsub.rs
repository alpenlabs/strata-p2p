//! Message type for GossipSub Message.

use libp2p::identity::PublicKey;
use serde::{Deserialize, Serialize};

use crate::swarm::{
    dto::{
        ProtocolId,
        signed::{HasAppPublicKey, SignedMessage},
    },
    serializing::pubkey_serialization::pubkey_serializer,
};

/// GossipSub protocol version enum.
#[repr(u8)]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GossipSubProtocolVersion {
    /// first version.
    V1,
    /// second (current last) version.
    V2,
}

/// Gossipsub message structure for the gossipsub protocol.
/// Serialized/deserialized using JSON format.
#[cfg(feature = "gossipsub")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GossipMessage {
    /// Protocol version.
    pub version: GossipSubProtocolVersion,
    /// Protocol identifier (gossipsub).
    pub protocol: ProtocolId,
    /// Gossipsub message data
    pub message: Vec<u8>,
    /// The application public key (Ed25519).
    #[serde(with = "pubkey_serializer")]
    pub app_public_key: PublicKey,
    /// Timestamp of message creation.
    pub date: u64,
}

#[cfg(feature = "gossipsub")]
impl GossipMessage {
    /// Creates a new gossipsub message with the given parameters.
    pub(crate) fn new(app_public_key: PublicKey, message: Vec<u8>) -> Self {
        use crate::swarm::dto::get_timestamp;

        let timestamp = get_timestamp();

        Self {
            version: GossipSubProtocolVersion::V2,
            protocol: ProtocolId::Gossip,
            message,
            app_public_key,
            date: timestamp,
        }
    }
}

impl HasAppPublicKey for GossipMessage {
    fn app_public_key(&self) -> &PublicKey {
        &self.app_public_key
    }
}

/// Type alias for signed gossipsub message.
pub type SignedGossipsubMessage = SignedMessage<GossipMessage>;
