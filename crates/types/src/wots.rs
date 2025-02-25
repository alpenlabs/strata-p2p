//! WOTS variable-length public keys.

use core::fmt;
use std::ops::{Deref, DerefMut};

#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serde::{
    de::{self, SeqAccess, Visitor},
    ser::{Serialize, SerializeSeq, Serializer},
    Deserialize, Deserializer,
};

/// A single Winternitz One-Time Signature (WOTS) hash value.
pub const WOTS_SINGLE: usize = 20;

/// A variable-length Winternitz One-Time Signature (WOTS) public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct WotsPublicKey<const MSG_LEN: usize>(pub [[u8; WOTS_SINGLE]; 2 * MSG_LEN + 4])
where
    [(); 2 * MSG_LEN + 4]: Sized;

/// 160-bit Winternitz One-Time Signature (WOTS) public key.
pub type Wots160PublicKey = WotsPublicKey<20>;

/// 256-bit Winternitz One-Time Signature (WOTS) public key.
pub type Wots256PublicKey = WotsPublicKey<32>;

impl<const MSG_LEN: usize> WotsPublicKey<MSG_LEN>
where
    [(); 2 * MSG_LEN + 4]: Sized,
    [(); WOTS_SINGLE * (2 * MSG_LEN + 4)]: Sized,
{
    /// The size of this WOTS public key in bytes.
    pub const SIZE: usize = 2 * MSG_LEN + 4;

    /// Creates a new WOTS 160-bit public key from a byte array.
    pub fn new(bytes: [[u8; WOTS_SINGLE]; 2 * MSG_LEN + 4]) -> Self {
        Self(bytes)
    }

    /// Converts the public key to a byte array.
    pub fn to_bytes(&self) -> [[u8; WOTS_SINGLE]; 2 * MSG_LEN + 4] {
        self.0
    }

    /// Converts the public key to a flattened byte array.
    pub fn to_flattened_bytes(&self) -> Vec<u8> {
        // Changed return type to Vec<u8>
        let mut bytes = Vec::with_capacity(WOTS_SINGLE * (2 * MSG_LEN + 4));
        for byte_array in &self.0 {
            bytes.extend_from_slice(byte_array);
        }
        bytes
    }

    /// Creates the public key from a flattened byte array.
    ///
    /// If you already have a structured `[[u8; 20]; MSG_LEN]` then you should use
    /// [`WotsPublicKey::new`].
    ///
    /// # Panics
    ///
    /// Panics if the byte array is not of proper length.
    pub fn from_flattened_bytes(bytes: &[u8]) -> Self {
        assert_eq!(
            bytes.len(),
            WOTS_SINGLE * (2 * MSG_LEN + 4),
            "Invalid byte array length"
        );

        let mut key = [[0u8; WOTS_SINGLE]; 2 * MSG_LEN + 4];
        for (i, byte_array) in key.iter_mut().enumerate() {
            byte_array.copy_from_slice(&bytes[i * WOTS_SINGLE..(i + 1) * WOTS_SINGLE]);
        }
        Self(key)
    }
}

impl<const MSG_LEN: usize> Deref for WotsPublicKey<MSG_LEN>
where
    [(); 2 * MSG_LEN + 4]: Sized,
{
    type Target = [[u8; WOTS_SINGLE]; 2 * MSG_LEN + 4];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const MSG_LEN: usize> DerefMut for WotsPublicKey<MSG_LEN>
where
    [(); 2 * MSG_LEN + 4]: Sized,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// Custom Serialization for WotsPublicKey
impl<const MSG_LEN: usize> Serialize for WotsPublicKey<MSG_LEN>
where
    [(); 2 * MSG_LEN + 4]: Sized,
    [(); WOTS_SINGLE * (2 * MSG_LEN + 4)]: Sized,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            let mut seq = serializer.serialize_seq(Some(2 * MSG_LEN + 4))?;
            for byte_array in &self.0 {
                seq.serialize_element(byte_array)?;
            }
            seq.end()
        } else {
            serializer.serialize_bytes(&self.to_flattened_bytes())
        }
    }
}

// Custom Deserialization for WotsPublicKey
impl<'de, const MSG_LEN: usize> Deserialize<'de> for WotsPublicKey<MSG_LEN>
where
    [(); 2 * MSG_LEN + 4]: Sized,
    [(); WOTS_SINGLE * (2 * MSG_LEN + 4)]: Sized,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WotsPublicKeyVisitor<const M: usize>;

        impl<'de, const M: usize> Visitor<'de> for WotsPublicKeyVisitor<M>
        where
            [(); 2 * M + 4]: Sized,
        {
            type Value = WotsPublicKey<M>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str(&format!(
                    "a WotsPublicKey with {} bytes",
                    WOTS_SINGLE * (2 * M + 4)
                ))
            }

            fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                if bytes.len() != WOTS_SINGLE * (2 * M + 4) {
                    return Err(E::invalid_length(bytes.len(), &self));
                }

                let mut array = [[0u8; WOTS_SINGLE]; 2 * M + 4];
                for (i, chunk) in bytes.chunks(WOTS_SINGLE).enumerate() {
                    array[i].copy_from_slice(chunk);
                }
                Ok(WotsPublicKey(array))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut array = [[0u8; WOTS_SINGLE]; 2 * M + 4];
                for (i, item) in array.iter_mut().enumerate() {
                    *item = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                }
                if seq.next_element::<[u8; WOTS_SINGLE]>()?.is_some() {
                    return Err(de::Error::invalid_length(2 * M + 5, &self));
                }
                Ok(WotsPublicKey(array))
            }
        }

        if deserializer.is_human_readable() {
            deserializer.deserialize_seq(WotsPublicKeyVisitor::<MSG_LEN>)
        } else {
            deserializer.deserialize_bytes(WotsPublicKeyVisitor::<MSG_LEN>)
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "proptest")]
    use proptest::prelude::*;

    use super::*;

    // Sanity checks for constants
    #[test]
    fn sanity_check_constants() {
        assert_eq!(WOTS_SINGLE, 20);
    }

    #[test]
    fn flattened_bytes_roundtrip() {
        let key160 = Wots160PublicKey::new([[1u8; WOTS_SINGLE]; 44]); // 2 * 20 + 4
        let key256 = Wots256PublicKey::new([[1u8; WOTS_SINGLE]; 68]); // 2 * 32 + 4

        // Test flattened bytes roundtrip
        let flattened160 = key160.to_flattened_bytes();
        let flattened256 = key256.to_flattened_bytes();

        let deserialized160 = Wots160PublicKey::from_flattened_bytes(&flattened160);
        let deserialized256 = Wots256PublicKey::from_flattened_bytes(&flattened256);

        assert_eq!(key160, deserialized160);
        assert_eq!(key256, deserialized256);
    }

    #[test]
    fn json_serialization() {
        let key160 = Wots160PublicKey::new([[1u8; WOTS_SINGLE]; 44]); // 2 * 20 + 4
        let key256 = Wots256PublicKey::new([[1u8; WOTS_SINGLE]; 68]); // 2 * 32 + 4

        // Test JSON serialization
        let serialized160 = serde_json::to_string(&key160).unwrap();
        let serialized256 = serde_json::to_string(&key256).unwrap();
        let deserialized160: Wots160PublicKey = serde_json::from_str(&serialized160).unwrap();
        let deserialized256: Wots256PublicKey = serde_json::from_str(&serialized256).unwrap();

        assert_eq!(key160, deserialized160);
        assert_eq!(key256, deserialized256);
    }

    #[test]
    fn bincode_serialization() {
        let key160 = Wots160PublicKey::new([[1u8; WOTS_SINGLE]; 44]); // 2 * 20 + 4
        let key256 = Wots256PublicKey::new([[1u8; WOTS_SINGLE]; 68]); // 2 * 32 + 4

        // Test bincode serialization
        let serialized160 = bincode::serialize(&key160).unwrap();
        let deserialized160: Wots160PublicKey = bincode::deserialize(&serialized160).unwrap();

        let serialized256 = bincode::serialize(&key256).unwrap();
        let deserialized256: Wots256PublicKey = bincode::deserialize(&serialized256).unwrap();

        assert_eq!(key160, deserialized160);
        assert_eq!(key256, deserialized256);
    }

    #[test]
    #[should_panic]
    fn deserialize_too_few_elements() {
        let json = "[[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]]"; // Only one array
        let _: Wots160PublicKey = serde_json::from_str(json).unwrap();
        let _: Wots256PublicKey = serde_json::from_str(json).unwrap();
    }

    #[test]
    #[should_panic]
    fn deserialize_too_many_elements20() {
        // Create JSON string with 45 arrays
        let mut json = String::from("[");
        for i in 0..45 {
            json.push_str("[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]");
            if i < 44 {
                json.push(',');
            }
        }
        json.push(']');

        let _: Wots160PublicKey = serde_json::from_str(&json).unwrap();
    }

    #[test]
    #[should_panic]
    fn deserialize_too_many_elements32() {
        // Create JSON string with 33 arrays
        let mut json = String::from("[");
        for i in 0..33 {
            json.push_str("[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]");
            if i < 32 {
                json.push(',');
            }
        }
        json.push(']');

        let _: Wots256PublicKey = serde_json::from_str(&json).unwrap();
    }

    #[test]
    #[should_panic]
    fn deserialize_invalid_array_length20() {
        // Create JSON with one array having wrong length (19 instead of 20)
        let mut json = String::from("[");
        for i in 0..WotsPublicKey::<20>::SIZE {
            if i == 14 {
                json.push_str("[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]"); // 19 elements
            } else {
                json.push_str("[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]");
            }
            if i < WotsPublicKey::<20>::SIZE - 1 {
                json.push(',');
            }
        }
        json.push(']');

        let _: Wots160PublicKey = serde_json::from_str(&json).unwrap();
    }

    #[test]
    #[should_panic]
    fn deserialize_invalid_array_length32() {
        // Create JSON with one array having wrong length (19 instead of 20)
        let mut json = String::from("[");
        for i in 0..WotsPublicKey::<32>::SIZE {
            if i == 20 {
                json.push_str("[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]"); // 19 elements
            } else {
                json.push_str("[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]");
            }
            if i < WotsPublicKey::<32>::SIZE - 1 {
                json.push(',');
            }
        }
        json.push(']');

        let _: Wots256PublicKey = serde_json::from_str(&json).unwrap();
    }

    #[cfg(feature = "proptest")]
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1_000))]

        #[test]
        fn proptest_serde_json_roundtrip160(key: Wots160PublicKey) {
            let serialized = serde_json::to_string(&key).unwrap();
            let deserialized: Wots160PublicKey = serde_json::from_str(&serialized).unwrap();
            prop_assert_eq!(key, deserialized);
        }

        #[test]
        fn proptest_serde_json_roundtrip256(key: Wots256PublicKey) {
            let serialized = serde_json::to_string(&key).unwrap();
            let deserialized: Wots256PublicKey = serde_json::from_str(&serialized).unwrap();
            prop_assert_eq!(key, deserialized);
        }

        #[test]
        fn proptest_bincode_roundtrip160(key: Wots160PublicKey) {
            let serialized = bincode::serialize(&key).unwrap();
            let deserialized: Wots160PublicKey = bincode::deserialize(&serialized).unwrap();
            prop_assert_eq!(key, deserialized);
        }

        #[test]
        fn proptest_bincode_roundtrip256(key: Wots256PublicKey) {
            let serialized = bincode::serialize(&key).unwrap();
            let deserialized: Wots256PublicKey = bincode::deserialize(&serialized).unwrap();
            prop_assert_eq!(key, deserialized);
        }

        #[test]
        fn proptest_deref_operations160(key: Wots160PublicKey) {
            let mut key_copy = key;

            // Test Deref
            prop_assert_eq!(&key.0, &*key);

            // Test DerefMut
            (*key_copy)[0] = [42u8; WOTS_SINGLE];
            prop_assert_eq!(key_copy.0[0], [42u8; WOTS_SINGLE]);
        }

        #[test]
        fn proptest_deref_operations256(key: Wots256PublicKey) {
            let mut key_copy = key;

            // Test Deref
            prop_assert_eq!(&key.0, &*key);

            // Test DerefMut
            (*key_copy)[0] = [42u8; WOTS_SINGLE];
            prop_assert_eq!(key_copy.0[0], [42u8; WOTS_SINGLE]);
        }

        #[test]
        fn proptest_new_constructor160(bytes: [[u8; WOTS_SINGLE]; 44]) { // 2 * 20 + 4
            let key = Wots160PublicKey::new(bytes);
            prop_assert_eq!(key.0, bytes);
        }

        #[test]
        fn proptest_new_constructor256(bytes: [[u8; WOTS_SINGLE]; 68]) { // 2 * 32 + 4
            let key = Wots256PublicKey::new(bytes);
            prop_assert_eq!(key.0, bytes);
        }

        #[test]
        fn proptest_to_flattened_bytes160(key: Wots160PublicKey) {
            let flattened = key.to_flattened_bytes();

            // Verify the length is correct
            prop_assert_eq!(flattened.len(), WOTS_SINGLE * (2 * 20 + 4));

            // Verify each segment matches the original arrays
            for (i, original_array) in key.0.iter().enumerate() {
                let segment = &flattened[i * WOTS_SINGLE..(i + 1) * WOTS_SINGLE];
                prop_assert_eq!(segment, original_array.as_slice());
            }
        }

        #[test]
        fn proptest_to_flattened_bytes256(key: Wots256PublicKey) {
            let flattened = key.to_flattened_bytes();

            // Verify the length is correct
            prop_assert_eq!(flattened.len(), WOTS_SINGLE * (2 * 32 + 4));

            // Verify each segment matches the original arrays
            for (i, original_array) in key.0.iter().enumerate() {
                let segment = &flattened[i * WOTS_SINGLE..(i + 1) * WOTS_SINGLE];
                prop_assert_eq!(segment, original_array.as_slice());
            }
        }
    }
}
