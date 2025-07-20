//! Message types for P2P protocol communication.
//!
//! This module provides an extensible message system that supports different
//! message types for the P2P protocol, including setup and gossip messages.

use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

use libp2p::{PeerId, identity::PublicKey};

/// Protocol version for all messages
pub const PROTOCOL_VERSION: u8 = 2;

/// Protocol identifier for setup messages
pub const SETUP_PROTOCOL_ID: u8 = 1;

/// Enum representing different message types in the P2P protocol
#[derive(Debug, Clone)]
pub enum Message {
    /// Setup message for handshake protocol
    Setup(SetupMessage),
}

/// Trait for all P2P messages that can be signed and verified
pub trait P2PMessage {
    /// Gets the protocol identifier for this message type
    fn protocol(&self) -> u8;

    /// Gets the protocol version
    fn version(&self) -> u8;

    /// Creates the message content for signing (excludes signature field)
    fn message_for_signing(&self) -> Vec<u8>;

    /// Gets the signature bytes
    fn signature(&self) -> &[u8];

    /// Sets the signature bytes
    fn set_signature(&mut self, signature: Vec<u8>);

    /// Validates the message format and content
    fn validate(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Serializes the message to bytes (manual serialization)
    fn to_bytes(&self) -> Vec<u8>;

    /// Deserializes the message from bytes (manual deserialization)
    fn from_bytes(data: &[u8]) -> Result<(Self, Vec<u8>), io::Error>
    where
        Self: Sized;
}

impl Message {
    /// Creates a new setup message
    pub fn setup(
        app_public_key: &PublicKey,
        local_peer_id: PeerId,
        remote_peer_id: PeerId,
    ) -> Self {
        Message::Setup(SetupMessage::new(
            app_public_key,
            local_peer_id,
            remote_peer_id,
        ))
    }
}

/// Setup message structure for the handshake protocol.
/// Format: [version][protocol(setup)][application_pk][transport_id_my][transport_id_their][unix_timestamp]
#[derive(Debug, Clone, PartialEq)]
pub struct SetupMessage {
    /// Protocol version
    pub version: u8,
    /// Protocol identifier (setup)
    pub protocol: u8,
    /// The application public key encoded as bytes
    pub app_public_key: Vec<u8>,
    /// My transport ID (PeerId)
    pub transport_id_my: Vec<u8>,
    /// Their transport ID (PeerId)
    pub transport_id_their: Vec<u8>,
    /// Timestamp of message creation
    pub date: u64,
    /// Signature of the message content
    pub signature: Vec<u8>,
}

impl SetupMessage {
    /// Creates a new setup message with the given parameters.
    pub fn new(app_public_key: &PublicKey, local_peer_id: PeerId, remote_peer_id: PeerId) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            version: PROTOCOL_VERSION,
            protocol: SETUP_PROTOCOL_ID,
            app_public_key: app_public_key.encode_protobuf(),
            transport_id_my: local_peer_id.to_bytes(),
            transport_id_their: remote_peer_id.to_bytes(),
            date: timestamp,
            signature: Vec::new(),
        }
    }

    /// Gets the application public key as a libp2p PublicKey.
    pub fn get_app_public_key(
        &self,
    ) -> Result<PublicKey, Box<dyn std::error::Error + Send + Sync>> {
        PublicKey::try_decode_protobuf(&self.app_public_key)
            .map_err(|e| format!("Failed to decode application public key: {}", e).into())
    }

    /// Gets the local peer ID.
    pub fn get_local_peer_id(&self) -> Result<PeerId, Box<dyn std::error::Error + Send + Sync>> {
        PeerId::from_bytes(&self.transport_id_my)
            .map_err(|e| format!("Failed to decode local peer ID: {}", e).into())
    }

    /// Gets the remote peer ID.
    pub fn get_remote_peer_id(&self) -> Result<PeerId, Box<dyn std::error::Error + Send + Sync>> {
        PeerId::from_bytes(&self.transport_id_their)
            .map_err(|e| format!("Failed to decode remote peer ID: {}", e).into())
    }
}

impl P2PMessage for SetupMessage {
    fn protocol(&self) -> u8 {
        self.protocol
    }

    fn version(&self) -> u8 {
        self.version
    }

    fn message_for_signing(&self) -> Vec<u8> {
        let mut content = Vec::new();
        content.push(self.version);
        content.push(self.protocol);
        content.extend_from_slice(&self.app_public_key);
        content.extend_from_slice(&self.transport_id_my);
        content.extend_from_slice(&self.transport_id_their);
        content.extend_from_slice(&self.date.to_be_bytes());
        content
    }

    fn signature(&self) -> &[u8] {
        &self.signature
    }

    fn set_signature(&mut self, signature: Vec<u8>) {
        self.signature = signature;
    }

    fn validate(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.version != PROTOCOL_VERSION {
            return Err(format!("Invalid protocol version: {}", self.version).into());
        }

        if self.protocol != SETUP_PROTOCOL_ID {
            return Err(format!("Invalid protocol ID: {}", self.protocol).into());
        }

        if self.app_public_key.is_empty() {
            return Err("Application public key is empty".into());
        }

        if self.transport_id_my.is_empty() {
            return Err("Local transport ID is empty".into());
        }

        if self.transport_id_their.is_empty() {
            return Err("Remote transport ID is empty".into());
        }

        if self.date == 0 {
            return Err("Invalid timestamp".into());
        }

        Ok(())
    }

    fn to_bytes(&self) -> Vec<u8> {
        // Manual serialization for SetupMessage
        let mut bytes = Vec::new();

        // Write version (1 byte)
        bytes.push(self.version);

        // Write protocol (1 byte)
        bytes.push(self.protocol);

        // Write app_public_key length and data
        bytes.extend_from_slice(&(self.app_public_key.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.app_public_key);

        // Write transport_id_my length and data
        bytes.extend_from_slice(&(self.transport_id_my.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.transport_id_my);

        // Write transport_id_their length and data
        bytes.extend_from_slice(&(self.transport_id_their.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.transport_id_their);

        // Write date (8 bytes)
        bytes.extend_from_slice(&self.date.to_be_bytes());

        // Write signature length and data
        bytes.extend_from_slice(&(self.signature.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.signature);

        bytes
    }

    fn from_bytes(data: &[u8]) -> Result<(Self, Vec<u8>), io::Error> {
        let mut cursor = 0;

        if data.len() < 1 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short for version"));
        }

        // Read version
        let version = data[cursor];
        cursor += 1;

        // Read protocol
        if data.len() < cursor + 1 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short for protocol"));
        }
        let protocol = data[cursor];
        cursor += 1;

        // Read app_public_key
        if data.len() < cursor + 4 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short for app_public_key length"));
        }
        let app_key_len = u32::from_be_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]) as usize;
        cursor += 4;

        if data.len() < cursor + app_key_len {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short for app_public_key"));
        }
        let app_public_key = data[cursor..cursor + app_key_len].to_vec();
        cursor += app_key_len;

        // Read transport_id_my
        if data.len() < cursor + 4 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short for transport_id_my length"));
        }
        let my_id_len = u32::from_be_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]) as usize;
        cursor += 4;

        if data.len() < cursor + my_id_len {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short for transport_id_my"));
        }
        let transport_id_my = data[cursor..cursor + my_id_len].to_vec();
        cursor += my_id_len;

        // Read transport_id_their
        if data.len() < cursor + 4 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short for transport_id_their length"));
        }
        let their_id_len = u32::from_be_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]) as usize;
        cursor += 4;

        if data.len() < cursor + their_id_len {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short for transport_id_their"));
        }
        let transport_id_their = data[cursor..cursor + their_id_len].to_vec();
        cursor += their_id_len;

        // Read date
        if data.len() < cursor + 8 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short for date"));
        }
        let date = u64::from_be_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
            data[cursor + 4],
            data[cursor + 5],
            data[cursor + 6],
            data[cursor + 7],
        ]);
        cursor += 8;

        // Read signature
        if data.len() < cursor + 4 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short for signature length"));
        }
        let sig_len = u32::from_be_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]) as usize;
        cursor += 4;

        if data.len() < cursor + sig_len {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short for signature"));
        }
        let signature = data[cursor..cursor + sig_len].to_vec();

        let message = SetupMessage {
            version,
            protocol,
            app_public_key,
            transport_id_my,
            transport_id_their,
            date,
            signature,
        };
        
        // Return the message and remaining data
        let remaining_data = if cursor + sig_len < data.len() {
            data[cursor + sig_len..].to_vec()
        } else {
            Vec::new()
        };
        
        Ok((message, remaining_data))
    }
}
