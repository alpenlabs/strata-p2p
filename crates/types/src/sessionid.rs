use bitcoin::hashes::{sha256, Hash};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Deserialize, Serialize)]
pub struct SessionId(sha256::Hash);

impl SessionId {
    pub const fn hash(data: &[u8]) -> Self {
        Self(sha256::Hash::const_hash(data))
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_byte_array().to_vec()
    }
}

impl AsRef<[u8]> for SessionId {
    fn as_ref(&self) -> &[u8] {
        self.0.as_byte_array().as_ref()
    }
}

impl From<sha256::Hash> for SessionId {
    fn from(value: sha256::Hash) -> Self {
        Self(value)
    }
}
