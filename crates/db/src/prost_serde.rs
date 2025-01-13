use serde::{Deserialize, Deserializer, Serializer, de::Error};

pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
where
    T: prost::Message,
    S: Serializer,
{
    serializer.serialize_bytes(&value.encode_to_vec())
}

pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: prost::Message + Default,
{
    let bytes = Vec::<u8>::deserialize(deserializer)?;
    let msg = T::decode(bytes.as_slice()).map_err(Error::custom)?;

    Ok(msg)
}
