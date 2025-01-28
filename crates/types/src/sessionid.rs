use core::fmt;

use bitcoin::hashes::{sha256, Hash};
use serde::{Deserialize, Serialize};

/// A unique identifier of signatures and nonces exchange session made by
/// hashing unique data related to it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Deserialize, Serialize)]
pub struct SessionId([u8; SessionId::SIZE]);

impl SessionId {
    const SIZE: usize = 32;

    /// Construct session ID from preimage by hashing it.
    pub fn hash(data: &[u8]) -> Self {
        Self(sha256::Hash::const_hash(data).to_byte_array())
    }

    pub fn from_bytes(bytes: [u8; Self::SIZE]) -> Self {
        Self(bytes)
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl AsRef<[u8]> for SessionId {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<sha256::Hash> for SessionId {
    fn from(value: sha256::Hash) -> Self {
        Self(value.to_byte_array())
    }
}
