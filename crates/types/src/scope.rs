use core::fmt;

use bitcoin::hashes::{sha256, Hash};
use serde::{Deserialize, Serialize};

/// A unique identifier of deposit setup made by hashing unique data related to it
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Deserialize, Serialize)]
pub struct Scope([u8; Scope::SIZE]);

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

impl Scope {
    const SIZE: usize = 32;

    /// Construct scope from preimage by hashing it.
    pub fn hash(data: &[u8]) -> Self {
        Self(sha256::Hash::hash(data).to_byte_array())
    }

    /// Construct scope from array of bytes.
    pub fn from_bytes(bytes: [u8; Self::SIZE]) -> Self {
        Self(bytes)
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl AsRef<[u8]> for Scope {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl From<sha256::Hash> for Scope {
    fn from(value: sha256::Hash) -> Self {
        Self(value.to_byte_array())
    }
}
