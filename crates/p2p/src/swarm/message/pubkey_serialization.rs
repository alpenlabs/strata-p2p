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
        let bytes: Vec<u8> = serde::Deserialize::deserialize(deserializer)?;
        PublicKey::try_decode_protobuf(&bytes)
            .map_err(|_| de::Error::custom("Failed to deserialize pubkey"))
    }
}
