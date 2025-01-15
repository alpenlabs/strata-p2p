use libp2p_identity::secp256k1::PublicKey;
use serde::{Deserialize, Deserializer, Serializer, de::Error};

pub fn serialize<S>(key: &PublicKey, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let key_bytes = key.to_bytes();
    serializer.serialize_bytes(&key_bytes)
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<PublicKey, D::Error>
where
    D: Deserializer<'de>,
{
    let key_bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
    PublicKey::try_from_bytes(&key_bytes).map_err(Error::custom)
}
