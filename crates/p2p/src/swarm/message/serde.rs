//! Serialization and deserialization of messages.

use libp2p::identity::PublicKey;

pub(crate) mod pubkey_serializer {
    use serde::{self, Deserializer, Serializer, de, ser::SerializeSeq};

    use super::PublicKey;

    pub(crate) fn serialize<S>(data: &PublicKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = data.encode_protobuf();
        let mut seq = serializer.serialize_seq(Some(bytes.len()))?;
        for byte in &bytes {
            seq.serialize_element(byte)?;
        }
        seq.end()
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<PublicKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        PublicKey::try_decode_protobuf(&bytes)
            .map_err(|_| de::Error::custom("Failed to deserialize pubkey"))
    }
}

pub(crate) mod signature_serializer {
    use serde::{self, Deserializer, Serializer, de, ser::SerializeSeq};

    pub(crate) fn serialize<S>(data: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(64))?;
        for byte in data {
            seq.serialize_element(byte)?;
        }
        seq.end()
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        bytes
            .try_into()
            .map_err(|_| de::Error::custom("Signature must be exactly 64 bytes"))
    }
}
