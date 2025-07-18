//! Message types for P2P protocol communication.
//!
//! This module provides an extensible message system that supports different
//! message types for the P2P protocol, including setup and gossip messages.

use std::{
    io,
    time::{SystemTime, UNIX_EPOCH},
};

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

    /// Serializes the message to bytes (manual serialization)
    fn to_bytes(&self) -> Vec<u8>;

    /// Deserializes the message from bytes (manual deserialization)
    fn from_bytes(data: &[u8]) -> Result<Self, io::Error>
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

    /// Creates a new signed setup message with the given signer
    pub fn setup_signed<S: crate::signer::ApplicationSigner>(
        app_public_key: &PublicKey,
        local_peer_id: PeerId,
        remote_peer_id: PeerId,
        signer: &S,
    ) -> Result<(Self, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
        // Create the unsigned message first
        let setup_message = SetupMessage::new(app_public_key, local_peer_id, remote_peer_id);

        // Get the message bytes for signing
        let message_bytes = setup_message.to_bytes();

        // Sign the message
        let signature = signer.sign(&message_bytes, app_public_key)?;

        Ok((Message::Setup(setup_message), signature))
    }

    /// Verifies a message signature
    pub fn verify_signature(
        &self,
        signature: &[u8],
    ) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        match self {
            Message::Setup(setup_msg) => {
                let app_public_key = setup_msg.get_app_public_key()?;
                let message_bytes = setup_msg.to_bytes();

                Ok(app_public_key.verify(&message_bytes, signature))
            }
        }
    }

    /// Extracts the message content (without signature) as bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Message::Setup(setup_msg) => setup_msg.to_bytes(),
        }
    }

    /// Deserializes a message from bytes, returning both the message and signature
    pub fn from_bytes(data: &[u8]) -> Result<(Self, Vec<u8>), io::Error> {
        // For now, we assume it's a setup message based on the protocol ID
        if data.len() < 2 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Data too short"));
        }

        let protocol_id = data[1]; // Second byte is protocol ID

        match protocol_id {
            SETUP_PROTOCOL_ID => {
                let setup_msg = SetupMessage::from_bytes(data)?;
                let signature = setup_msg.signature.clone();
                Ok((Message::Setup(setup_msg), signature))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown protocol ID",
            )),
        }
    }
}

/// Setup message structure for the handshake protocol.
/// Format: \[version\]\[protocol(setup)\]\[application_pk\]\[local_transport_id\]\
/// [remote_transport_id\]\[unix_timestamp\]
#[derive(Debug, Clone, PartialEq)]
pub struct SetupMessage {
    /// Protocol version
    pub version: u8,
    /// Protocol identifier (setup)
    pub protocol: u8,
    /// The application public key encoded as bytes
    pub app_public_key: Vec<u8>,
    /// My transport ID (PeerId)
    pub local_transport_id: Vec<u8>,
    /// Their transport ID (PeerId)
    pub remote_transport_id: Vec<u8>,
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
            local_transport_id: local_peer_id.to_bytes(),
            remote_transport_id: remote_peer_id.to_bytes(),
            date: timestamp,
            signature: Vec::new(),
        }
    }

    /// Gets the application public key as a libp2p PublicKey.
    pub fn get_app_public_key(
        &self,
    ) -> Result<PublicKey, Box<dyn std::error::Error + Send + Sync>> {
        PublicKey::try_decode_protobuf(&self.app_public_key)
            .map_err(|e| format!("Failed to decode application public key: {e}").into())
    }

    /// Gets the local peer ID.
    pub fn get_local_peer_id(&self) -> Result<PeerId, Box<dyn std::error::Error + Send + Sync>> {
        PeerId::from_bytes(&self.local_transport_id)
            .map_err(|e| format!("Failed to decode local peer ID: {e}").into())
    }

    /// Gets the remote peer ID.
    pub fn get_remote_peer_id(&self) -> Result<PeerId, Box<dyn std::error::Error + Send + Sync>> {
        PeerId::from_bytes(&self.remote_transport_id)
            .map_err(|e| format!("Failed to decode remote peer ID: {e}").into())
    }

    /// Verifies the signature of this message using the embedded app public key.
    pub fn verify_signature(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        // Get the app public key
        let app_public_key = self.get_app_public_key()?;

        // Use the same method as signing - to_bytes() which excludes signature
        let content = self.to_bytes();

        // Verify the signature using libp2p's verification
        Ok(app_public_key.verify(&content, &self.signature))
    }

    /// Gets bytes with signature for transmission over the wire
    pub fn to_bytes_with_signature(&self) -> Vec<u8> {
        let mut bytes = self.to_bytes();

        // Write signature length and data
        bytes.extend_from_slice(&(self.signature.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.signature);

        bytes
    }

    /// Validates the message format and content
    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.version != PROTOCOL_VERSION {
            return Err(format!("Invalid protocol version: {}", self.version).into());
        }

        if self.protocol != SETUP_PROTOCOL_ID {
            return Err(format!("Invalid protocol ID: {}", self.protocol).into());
        }

        if self.app_public_key.is_empty() {
            return Err("Application public key is empty".into());
        }

        if self.local_transport_id.is_empty() {
            return Err("Local transport ID is empty".into());
        }

        if self.remote_transport_id.is_empty() {
            return Err("Remote transport ID is empty".into());
        }

        Ok(())
    }
}

impl P2PMessage for SetupMessage {
    fn protocol(&self) -> u8 {
        self.protocol
    }

    fn version(&self) -> u8 {
        self.version
    }

    fn to_bytes(&self) -> Vec<u8> {
        // Manual serialization for SetupMessage (without signature for signing purposes)
        let mut bytes = Vec::new();

        // Write version (1 byte)
        bytes.push(self.version);

        // Write protocol (1 byte)
        bytes.push(self.protocol);

        // Write app_public_key length and data
        bytes.extend_from_slice(&(self.app_public_key.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.app_public_key);

        // Write local_transport_id length and data
        bytes.extend_from_slice(&(self.local_transport_id.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.local_transport_id);

        // Write remote_transport_id length and data
        bytes.extend_from_slice(&(self.remote_transport_id.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.remote_transport_id);

        // Write date (8 bytes)
        bytes.extend_from_slice(&self.date.to_be_bytes());

        bytes
    }

    fn from_bytes(data: &[u8]) -> Result<Self, io::Error> {
        let mut cursor = 0;

        if data.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for version",
            ));
        }

        // Read version
        let version = data[cursor];
        cursor += 1;

        // Read protocol
        if data.len() < cursor + 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for protocol",
            ));
        }
        let protocol = data[cursor];
        cursor += 1;

        // Read app_public_key
        if data.len() < cursor + 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for app_public_key length",
            ));
        }
        let app_key_len = u32::from_be_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]) as usize;
        cursor += 4;

        if data.len() < cursor + app_key_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for app_public_key",
            ));
        }
        let app_public_key = data[cursor..cursor + app_key_len].to_vec();
        cursor += app_key_len;

        // Read local_transport_id
        if data.len() < cursor + 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for local_transport_id length",
            ));
        }
        let my_id_len = u32::from_be_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]) as usize;
        cursor += 4;

        if data.len() < cursor + my_id_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for local_transport_id",
            ));
        }
        let local_transport_id = data[cursor..cursor + my_id_len].to_vec();
        cursor += my_id_len;

        // Read remote_transport_id
        if data.len() < cursor + 4 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for remote_transport_id length",
            ));
        }
        let their_id_len = u32::from_be_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]) as usize;
        cursor += 4;

        if data.len() < cursor + their_id_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for remote_transport_id",
            ));
        }
        let remote_transport_id = data[cursor..cursor + their_id_len].to_vec();
        cursor += their_id_len;

        // Read date
        if data.len() < cursor + 8 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for date",
            ));
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
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for signature length",
            ));
        }
        let sig_len = u32::from_be_bytes([
            data[cursor],
            data[cursor + 1],
            data[cursor + 2],
            data[cursor + 3],
        ]) as usize;
        cursor += 4;

        if data.len() < cursor + sig_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Data too short for signature",
            ));
        }
        let signature = data[cursor..cursor + sig_len].to_vec();

        let message = SetupMessage {
            version,
            protocol,
            app_public_key,
            local_transport_id,
            remote_transport_id,
            date,
            signature,
        };

        Ok(message)
    }
}
