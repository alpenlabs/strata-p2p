//! Message type for GossipSub Message.

#![cfg(feature = "gossipsub")]

use libp2p::identity::PublicKey;
use serde::{Deserialize, Serialize};

use crate::swarm::message::{
    ProtocolId, get_timestamp,
    serde::pubkey_serializer,
    signed::{HasPublicKey, SignedMessage},
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GossipMessage {
    /// Protocol version.
    pub version: GossipSubProtocolVersion,
    /// Protocol identifier (gossipsub).
    pub protocol: ProtocolId,
    /// Gossipsub message data
    pub message: Vec<u8>,
    /// The public key (Ed25519). Application public key if BYOS is enabled, otherwise transport
    /// public key.
    #[serde(with = "pubkey_serializer")]
    pub public_key: PublicKey,
    /// Timestamp of message creation.
    pub date: u64,
}

impl GossipMessage {
    /// Creates a new gossipsub message with the given parameters.
    pub(crate) fn new(app_public_key: PublicKey, message: Vec<u8>) -> Self {
        let timestamp = get_timestamp();

        Self {
            version: GossipSubProtocolVersion::V2,
            protocol: ProtocolId::Gossip,
            message,
            public_key: app_public_key,
            date: timestamp,
        }
    }
}

impl HasPublicKey for GossipMessage {
    fn public_key(&self) -> &PublicKey {
        &self.public_key
    }
}

/// Type alias for signed gossipsub message.
pub type SignedGossipsubMessage = SignedMessage<GossipMessage>;
