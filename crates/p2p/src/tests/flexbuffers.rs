//! Tests for flexbuffers serialization and deserialization of SignedMessages.

#[cfg(all(feature = "byos", feature = "gossipsub"))]
use asynchronous_codec::{Decoder, Encoder};
#[cfg(all(feature = "byos", feature = "gossipsub"))]
use bytes::{BufMut, BytesMut};
#[cfg(feature = "request-response")]
use libp2p::PeerId;
use libp2p::identity::Keypair;
use tracing::{error, info};

#[cfg(any(feature = "gossipsub", feature = "request-response"))]
use crate::swarm::message::ProtocolId;
#[cfg(feature = "gossipsub")]
use crate::swarm::message::gossipsub::{
    GossipMessage, GossipSubProtocolVersion, SignedGossipsubMessage,
};
#[cfg(feature = "request-response")]
use crate::swarm::message::request_response::{
    RequestMessage, RequestResponseProtocolVersion, ResponseMessage, SignedRequestMessage,
    SignedResponseMessage,
};
#[cfg(all(feature = "byos", feature = "gossipsub"))]
use crate::swarm::setup::flexbuffers_codec::FlexbuffersCodec;
use crate::tests::common::init_tracing;

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

#[cfg(all(feature = "gossipsub", feature = "byos"))]
#[test]
fn test_codec() {
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
        signature: [7u8; 64],
    };

    let mut codec: FlexbuffersCodec<SignedGossipsubMessage> = FlexbuffersCodec::new();
    let mut buf = BytesMut::new();

    codec
        .encode(signed_msg.clone(), &mut buf)
        .expect("encode ok");
    assert!(!buf.is_empty());

    let decoded = codec.decode(&mut buf).expect("decode ok");
    assert_eq!(decoded, Some(signed_msg));
    assert!(buf.is_empty());
}

#[cfg(all(feature = "gossipsub", feature = "byos"))]
#[test]
fn test_codec_decode_invalid_reader_error() {
    let mut codec: FlexbuffersCodec<SignedGossipsubMessage> = FlexbuffersCodec::new();
    let mut buf = BytesMut::new();
    let data = b"not-flexbuffers";
    buf.put_u32(data.len() as u32);
    buf.put_slice(data);

    let err = codec.decode(&mut buf).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[cfg(all(feature = "gossipsub", feature = "byos"))]
#[test]
fn test_codec_frame_size_limit_accepts_valid_size() {
    init_tracing();

    let keypair = Keypair::generate_ed25519();
    let public_key = keypair.public();

    // Create a message that's well within the 1MB limit
    let gossip_msg = GossipMessage {
        version: GossipSubProtocolVersion::V2,
        protocol: ProtocolId::Gossip,
        message: vec![0u8; 1024], // 1KB message
        public_key: public_key.clone(),
        date: 1234567890,
    };

    let signed_msg = SignedGossipsubMessage {
        message: gossip_msg,
        signature: [7u8; 64],
    };

    let mut codec: FlexbuffersCodec<SignedGossipsubMessage> = FlexbuffersCodec::new();
    let mut buf = BytesMut::new();

    // Encode the message
    codec
        .encode(signed_msg.clone(), &mut buf)
        .expect("encode should succeed");

    info!(buf_size = buf.len(), "encoded buffer size");
    assert!(!buf.is_empty());

    // Decode should succeed since it's within the limit
    let decoded = codec
        .decode(&mut buf)
        .expect("decode should succeed for valid size");
    assert_eq!(decoded, Some(signed_msg));
    assert!(buf.is_empty());
}

#[cfg(all(feature = "gossipsub", feature = "byos"))]
#[test]
fn test_codec_frame_size_limit_rejects_oversized_frame() {
    init_tracing();

    // Create a buffer that exceeds the MAX_FRAME_SIZE (1MB)
    const MAX_FRAME_SIZE: usize = 1_024 * 1_024; // 1MB
    // We use a length prefix that exceeds the limit
    let oversized_len = (MAX_FRAME_SIZE + 1) as u32;
    let mut buf = BytesMut::new();
    buf.put_u32(oversized_len);
    // Note: We don't need to provide the full payload because the size check happens
    // immediately after reading the length prefix.

    info!(
        buf_size = buf.len(),
        max_size = MAX_FRAME_SIZE,
        "testing oversized frame"
    );

    let mut codec: FlexbuffersCodec<SignedGossipsubMessage> = FlexbuffersCodec::new();

    // Decode should fail with InvalidData error due to size limit
    let err = codec.decode(&mut buf).unwrap_err();

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("exceeds maximum allowed size"),
        "error message should mention size limit, got: {}",
        err
    );
    info!(?err, "successfully rejected oversized frame");
}

#[cfg(all(feature = "gossipsub", feature = "byos"))]
#[test]
fn test_codec_frame_size_limit_boundary() {
    init_tracing();

    // Test at exactly the MAX_FRAME_SIZE boundary (1MB)
    const MAX_FRAME_SIZE: usize = 1_024 * 1_024; // 1MB

    // Create a buffer with length exactly at the max size
    let mut buf = BytesMut::with_capacity(4 + MAX_FRAME_SIZE);
    buf.put_u32(MAX_FRAME_SIZE as u32);
    // Add dummy data so it attempts to decode
    buf.resize(4 + MAX_FRAME_SIZE, 0);

    info!(
        buf_size = buf.len(),
        max_size = MAX_FRAME_SIZE,
        "testing boundary frame at max size"
    );

    let mut codec: FlexbuffersCodec<SignedGossipsubMessage> = FlexbuffersCodec::new();

    // This will fail at deserialization (not valid flexbuffers), but should pass the size check
    let result = codec.decode(&mut buf);

    // Should get InvalidData due to flexbuffers parsing error, not size limit error
    if let Err(err) = result {
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        // The error should be about reader/deserialization, not size
        assert!(
            !err.to_string().contains("exceeds maximum allowed size"),
            "should not fail on size check at boundary, got: {}",
            err
        );
        info!(
            ?err,
            "boundary test passed - failed on parsing, not size check"
        );
    }
}

#[cfg(all(feature = "gossipsub", feature = "byos"))]
#[test]
fn test_codec_handles_fragmentation() {
    init_tracing();

    let keypair = Keypair::generate_ed25519();
    let public_key = keypair.public();

    let gossip_msg = GossipMessage {
        version: GossipSubProtocolVersion::V2,
        protocol: ProtocolId::Gossip,
        message: b"test gossipsub message for fragmentation".to_vec(),
        public_key: public_key.clone(),
        date: 1234567890,
    };

    let signed_msg = SignedGossipsubMessage {
        message: gossip_msg,
        signature: [7u8; 64],
    };

    let mut codec: FlexbuffersCodec<SignedGossipsubMessage> = FlexbuffersCodec::new();
    let mut full_buf = BytesMut::new();

    // Encode the full message
    codec
        .encode(signed_msg.clone(), &mut full_buf)
        .expect("encode should succeed");

    // Split the buffer into two parts
    let split_point = full_buf.len() / 2;
    let part1 = &full_buf[..split_point];
    let part2 = &full_buf[split_point..];

    let mut decode_buf = BytesMut::new();

    // 1. Feed the first part
    decode_buf.put_slice(part1);
    let result = codec
        .decode(&mut decode_buf)
        .expect("decode should not error on partial data");
    assert!(result.is_none(), "should return None for partial data");

    // 2. Feed the second part
    decode_buf.put_slice(part2);
    let result = codec
        .decode(&mut decode_buf)
        .expect("decode should succeed on full data");

    assert_eq!(
        result,
        Some(signed_msg),
        "should decode correctly after reassembly"
    );
    assert!(
        decode_buf.is_empty(),
        "buffer should be empty after decoding"
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
        destination_peer_id: PeerId::random(),
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

#[cfg(feature = "kad")]
#[tokio::test]
async fn test_signed_dht_record_serialization() {
    use crate::swarm::message::{
        dht_record::{RecordData, SignedRecord},
        signed::HasPublicKey,
    };

    init_tracing();
    let keypair = Keypair::generate_ed25519();
    let public_key = keypair.public();

    let dht_record = RecordData::new(
        public_key.clone(),
        vec![
            "/ip4/127.0.0.1".parse().unwrap(),
            "/ip6/::1/tcp/0".parse().unwrap(),
        ],
    );

    let signed_record = SignedRecord {
        message: dht_record,
        signature: [42u8; 64], // Test signature
    };

    info!(?signed_record, "original signed response message");

    let serialized = match flexbuffers::to_vec(&signed_record) {
        Ok(data) => {
            info!(data_len = data.len(), "response serialization successful");
            data
        }
        Err(e) => {
            error!(?e, "response serialization failed");
            return;
        }
    };

    let deserialized: SignedRecord = match flexbuffers::from_slice(&serialized) {
        Ok(msg) => {
            info!("response deserialization successful");
            msg
        }
        Err(e) => {
            error!(?e, "response deserialization failed");
            return;
        }
    };

    assert_eq!(signed_record, deserialized, "Records should be identical");
    assert_eq!(
        public_key,
        deserialized.message.public_key().clone(),
        "Public key should be identical"
    );
}
