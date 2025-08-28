//! Serialization and deserialization of messages.

use libp2p::identity::PublicKey;

pub(crate) mod pubkey_serializer {
    use serde::{self, Deserializer, Serializer, de};

    use super::PublicKey;

    pub(crate) fn serialize<S>(data: &PublicKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&data.encode_protobuf())
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<PublicKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: &[u8] = serde::Deserialize::deserialize(deserializer)?;
        PublicKey::try_decode_protobuf(bytes)
            .map_err(|_| de::Error::custom("Failed to deserialize pubkey"))
    }
}

pub(crate) mod signature_serializer {
    use serde::{self, Deserializer, Serializer, de};

    pub(crate) fn serialize<S>(data: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(data)
    }

    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: &[u8] = serde::Deserialize::deserialize(deserializer)?;
        bytes
            .try_into()
            .map_err(|_| de::Error::custom("Signature must be exactly 64 bytes"))
    }
}
