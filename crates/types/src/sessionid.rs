use core::fmt;

use bitcoin::{
    consensus::{encode::Error, Decodable, Encodable},
    hashes::{sha256, Hash},
    io::Cursor,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Deserialize, Serialize)]
pub struct SessionId(sha256::Hash);

impl SessionId {
    pub const fn hash(data: &[u8]) -> Self {
        Self(sha256::Hash::const_hash(data))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let mut cursor = Cursor::new(bytes);
        let session_id = Decodable::consensus_decode(&mut cursor)?;
        Ok(Self(session_id))
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_byte_array().to_vec()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
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

impl Decodable for SessionId {
    fn consensus_decode<R: bitcoin::io::Read + ?Sized>(reader: &mut R) -> Result<Self, Error> {
        Ok(Self(sha256::Hash::consensus_decode(reader)?))
    }
}

impl Encodable for SessionId {
    fn consensus_encode<W: bitcoin::io::Write + ?Sized>(
        &self,
        writer: &mut W,
    ) -> Result<usize, bitcoin::io::Error> {
        self.0.consensus_encode(writer)
    }
}
