//! Types for Winternitz one-time signature (WOTS) public keys.
//!
//! Contrary to nonces, signatures, and public keys, WOTS keys cannot be uniquely identified
//! by a [`SessionId`](super::SessionId) (similar to Txid) alone. This is due the fact that you need
//! more than one WOTS key per transaction. Hence we need a secondary [`WotsId`] to handle this.

use std::io::{self, Cursor, Read};

/// Secondary identifier for WOTS keys.
pub type WotsId = u32;

/// Type alias for a SHA256 followed by a RIPEMD160 output.
///
/// This is what OP_HASH160 does in Bitcoin.
type Hash160 = [u8; 20];

/// 32-byte Winternitz one-time signature (WOTS) public key.
#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd,
)]
pub struct Wots32Key([Hash160; 32]);

/// 160-byte Winternitz one-time signature (WOTS) public key.
///
/// Used for hash elements.
// FIXME: Serde does not support `[[u8; 20]; 160]` derived `Serialize`.
#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd,
)]
pub struct Wots160Key(Vec<Hash160>);

/// 256-byte Winternitz one-time signature (WOTS) public key.
///
/// Used for field elements and public inputs.
// FIXME: Serde does not support `[[u8; 20]; 256]` derived `Serialize`.
#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd,
)]
pub struct Wots256Key(Vec<Hash160>);

impl Wots32Key {
    pub fn from_slice(bytes: &[u8]) -> Result<Self, io::Error> {
        if bytes.len() != 20 * 32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid byte length",
            ));
        }

        let mut cursor = Cursor::new(bytes);
        let mut keys = [[0; 20]; 32];

        for key in keys.iter_mut() {
            cursor.read_exact(key)?;
        }

        Ok(Wots32Key(keys))
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(20 * 32);
        for key in &self.0 {
            out.extend_from_slice(key);
        }
        out
    }
}

impl Wots160Key {
    pub fn from_slice(bytes: &[u8]) -> Result<Self, io::Error> {
        if bytes.len() != 20 * 160 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid byte length",
            ));
        }

        let mut cursor = Cursor::new(bytes);
        let mut keys = Vec::<Hash160>::with_capacity(160);

        for key in keys.iter_mut() {
            cursor.read_exact(key)?;
        }

        Ok(Wots160Key(keys))
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(20 * 160);
        for key in &self.0 {
            out.extend_from_slice(key);
        }
        out
    }
}

impl Wots256Key {
    pub fn from_slice(bytes: &[u8]) -> Result<Self, io::Error> {
        if bytes.len() != 20 * 256 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid byte length",
            ));
        }

        let mut cursor = Cursor::new(bytes);
        let mut keys = Vec::<Hash160>::with_capacity(256);

        for key in keys.iter_mut() {
            cursor.read_exact(key)?;
        }

        Ok(Wots256Key(keys))
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(20 * 256);
        for key in &self.0 {
            out.extend_from_slice(key);
        }
        out
    }
}

impl From<[Hash160; 32]> for Wots32Key {
    fn from(value: [Hash160; 32]) -> Self {
        Self(value)
    }
}

impl From<Wots32Key> for [Hash160; 32] {
    fn from(value: Wots32Key) -> Self {
        value.0
    }
}

impl From<Vec<Hash160>> for Wots160Key {
    fn from(value: Vec<Hash160>) -> Self {
        Self(value)
    }
}

impl From<Wots160Key> for Vec<Hash160> {
    fn from(value: Wots160Key) -> Self {
        value.0
    }
}

impl From<Vec<Hash160>> for Wots256Key {
    fn from(value: Vec<Hash160>) -> Self {
        Self(value)
    }
}

impl From<Wots256Key> for Vec<Hash160> {
    fn from(value: Wots256Key) -> Self {
        value.0
    }
}
