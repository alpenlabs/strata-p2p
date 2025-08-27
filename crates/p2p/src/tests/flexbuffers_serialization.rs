//! Tests for flexbuffers serialization and deserialization of SignedMessages.

use flexbuffers;
use libp2p::identity::Keypair;
use tracing::info;

use crate::{
    swarm::message::{
        ProtocolId,
        gossipsub::{GossipMessage, GossipSubProtocolVersion, SignedGossipsubMessage},
        request_response::{
            RequestMessage, RequestResponseProtocolVersion, ResponseMessage, SignedRequestMessage,
            SignedResponseMessage,
        },
    },
    tests::common::init_tracing,
};

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

    info!("Original signed gossipsub message: {:?}", signed_msg);

    let serialized = match flexbuffers::to_vec(&signed_msg) {
        Ok(data) => {
            info!(
                "Gossipsub serialization successful! Size: {} bytes",
                data.len()
            );
            data
        }
        Err(e) => {
            panic!("Gossipsub serialization failed: {:?}", e);
        }
    };

    let deserialized: SignedGossipsubMessage = match flexbuffers::from_slice(&serialized) {
        Ok(msg) => {
            info!("Gossipsub deserialization successful!");
            msg
        }
        Err(e) => {
            panic!("Gossipsub deserialization failed: {:?}", e);
        }
    };

    assert_eq!(
        signed_msg, deserialized,
        "Gossipsub messages should be identical"
    );
    info!("Gossipsub serialization/deserialization test passed!");
}

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

    info!("Original signed request message: {:?}", signed_msg);

    let serialized = match flexbuffers::to_vec(&signed_msg) {
        Ok(data) => {
            info!(
                "Request serialization successful! Size: {} bytes",
                data.len()
            );
            data
        }
        Err(e) => {
            panic!("Request serialization failed: {:?}", e);
        }
    };

    let deserialized: SignedRequestMessage = match flexbuffers::from_slice(&serialized) {
        Ok(msg) => {
            info!("Request deserialization successful!");
            msg
        }
        Err(e) => {
            panic!("Request deserialization failed: {:?}", e);
        }
    };

    assert_eq!(
        signed_msg, deserialized,
        "Request messages should be identical"
    );
    info!("Request serialization/deserialization test passed!");
}

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

    info!("Original signed response message: {:?}", signed_msg);

    let serialized = match flexbuffers::to_vec(&signed_msg) {
        Ok(data) => {
            info!(
                "Response serialization successful! Size: {} bytes",
                data.len()
            );
            data
        }
        Err(e) => {
            panic!("Response serialization failed: {:?}", e);
        }
    };

    let deserialized: SignedResponseMessage = match flexbuffers::from_slice(&serialized) {
        Ok(msg) => {
            info!("Response deserialization successful!");
            msg
        }
        Err(e) => {
            panic!("Response deserialization failed: {:?}", e);
        }
    };

    assert_eq!(
        signed_msg, deserialized,
        "Response messages should be identical"
    );
    info!("Response serialization/deserialization test passed!");
}
