//! Some structs that we use + serializing and deserializing.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Protocol identifiers for different message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ProtocolId {
    /// Setup protocol for peer handshake.
    Setup = 1,

    /// Gossipsub protocol for pub/sub messaging.
    #[cfg(feature = "gossipsub")]
    Gossip = 2,

    /// Request-response protocol for direct communication.
    #[cfg(feature = "request-response")]
    RequestResponse = 3,
}

impl From<ProtocolId> for u8 {
    fn from(protocol: ProtocolId) -> Self {
        protocol as u8
    }
}

impl TryFrom<u8> for ProtocolId {
    type Error = &'static str;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ProtocolId::Setup),

            #[cfg(feature = "gossipsub")]
            2 => Ok(ProtocolId::Gossip),

            #[cfg(feature = "request-response")]
            3 => Ok(ProtocolId::RequestResponse),

            _ => Err("Invalid protocol ID"),
        }
    }
}

fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub mod setup;
pub mod signed;

#[cfg(feature = "gossipsub")]
pub mod gossipsub;

#[cfg(feature = "request-response")]
pub mod request_response;
