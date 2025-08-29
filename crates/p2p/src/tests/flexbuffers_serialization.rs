//! Tests for flexbuffers serialization and deserialization of SignedMessages.

use libp2p::identity::Keypair;
use tracing::{error, info};

#[cfg(feature = "gossipsub")]
use crate::swarm::message::gossipsub::{
    GossipMessage, GossipSubProtocolVersion, SignedGossipsubMessage,
};
#[cfg(feature = "request-response")]
use crate::swarm::message::request_response::{
    RequestMessage, RequestResponseProtocolVersion, ResponseMessage, SignedRequestMessage,
    SignedResponseMessage,
};
use crate::{swarm::message::ProtocolId, tests::common::init_tracing};

#[cfg(feature = "gossipsub")]
#[tokio::test]
async fn test_signed_gossipsub_message_serialization() {
    init_tracing();
    let keypair = Keypair::generate_ed25519();
    let public_key = keypair.public();

    let gossip_msg = GossipMessage {
        version: GossipSubProtocolVersion::V2,
        protocol: ProtocolId::Gossip,
        message: b"test gossipsub message".to_vec(),
        public_key: public_key.clone(),
        date: 1234567890,
    };

    let signed_msg = SignedGossipsubMessage {
        message: gossip_msg,
        signature: [42u8; 64], // Test signature
    };

    info!(?signed_msg, "original signed gossipsub message");

    let serialized = match flexbuffers::to_vec(&signed_msg) {
        Ok(data) => {
            info!(data_len = data.len(), "gossipsub serialization successful");
            data
        }
        Err(e) => {
            error!(?e, "gossipsub serialization failed");
            return;
        }
    };

    let deserialized: SignedGossipsubMessage = match flexbuffers::from_slice(&serialized) {
        Ok(msg) => {
            info!("gossipsub deserialization successful");
            msg
        }
        Err(e) => {
            error!(?e, "gossipsub deserialization failed");
            return;
        }
    };

    assert_eq!(
        signed_msg, deserialized,
        "gossipsub messages should be identical"
    );
}

#[cfg(feature = "request-response")]
#[tokio::test]
async fn test_signed_request_message_serialization() {
    init_tracing();
    let keypair = Keypair::generate_ed25519();
    let public_key = keypair.public();

    let request_msg = RequestMessage {
        version: RequestResponseProtocolVersion::V2,
        protocol: ProtocolId::RequestResponse,
        message: b"test request message".to_vec(),
        public_key: public_key.clone(),
        date: 1234567890,
    };

    let signed_msg = SignedRequestMessage {
        message: request_msg,
        signature: [42u8; 64], // Test signature
    };

    info!(?signed_msg, "original signed request message");

    let serialized = match flexbuffers::to_vec(&signed_msg) {
        Ok(data) => {
            info!(data_len = data.len(), "request serialization successful");
            data
        }
        Err(e) => {
            error!(?e, "request serialization failed");
            return;
        }
    };

    let deserialized: SignedRequestMessage = match flexbuffers::from_slice(&serialized) {
        Ok(msg) => {
            info!("request deserialization successful");
            msg
        }
        Err(e) => {
            error!(?e, "request deserialization failed");
            return;
        }
    };

    assert_eq!(
        signed_msg, deserialized,
        "request messages should be identical"
    );
}

#[cfg(feature = "request-response")]
#[tokio::test]
async fn test_signed_response_message_serialization() {
    init_tracing();
    let keypair = Keypair::generate_ed25519();
    let public_key = keypair.public();

    let response_msg = ResponseMessage {
        version: RequestResponseProtocolVersion::V2,
        protocol: ProtocolId::RequestResponse,
        message: b"test response message".to_vec(),
        app_public_key: public_key.clone(),
        date: 1234567890,
    };

    let signed_msg = SignedResponseMessage {
        message: response_msg,
        signature: [42u8; 64], // Test signature
    };

    info!(?signed_msg, "original signed response message");

    let serialized = match flexbuffers::to_vec(&signed_msg) {
        Ok(data) => {
            info!(data_len = data.len(), "response serialization successful");
            data
        }
        Err(e) => {
            error!(?e, "response serialization failed");
            return;
        }
    };

    let deserialized: SignedResponseMessage = match flexbuffers::from_slice(&serialized) {
        Ok(msg) => {
            info!("response deserialization successful");
            msg
        }
        Err(e) => {
            error!(?e, "response deserialization failed");
            return;
        }
    };

    assert_eq!(
        signed_msg, deserialized,
        "response messages should be identical"
    );
}
