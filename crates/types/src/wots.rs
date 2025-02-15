//! WOTS 256-bit public key.

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

/// A 256-bit Winternitz One-Time Signature (WOTS) public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct Wots256PublicKey(pub [[u8; WOTS_SINGLE]; 256]);

impl Wots256PublicKey {
    /// Creates a new WOTS 256-bit public key from a byte array.
    pub fn new(bytes: [[u8; WOTS_SINGLE]; 256]) -> Self {
        Self(bytes)
    }

    /// Converts the public key to a byte array.
    pub fn to_bytes(&self) -> [[u8; WOTS_SINGLE]; 256] {
        self.0
    }

    /// Converts the public key to a flattened byte array.
    pub fn to_flattened_bytes(&self) -> [u8; WOTS_SINGLE * 256] {
        let mut bytes = [0u8; WOTS_SINGLE * 256];
        for (i, byte_array) in self.0.iter().enumerate() {
            bytes[i * WOTS_SINGLE..(i + 1) * WOTS_SINGLE].copy_from_slice(byte_array);
        }
        bytes
    }
}

impl Deref for Wots256PublicKey {
    type Target = [[u8; WOTS_SINGLE]; 256];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Wots256PublicKey {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Serialize for Wots256PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(256))?;
        for byte_array in &self.0 {
            seq.serialize_element(byte_array)?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for Wots256PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Wots256PublicKeyVisitor;

        impl<'de> Visitor<'de> for Wots256PublicKeyVisitor {
            type Value = Wots256PublicKey;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str(format!("exactly 256 elements of [u8; {WOTS_SINGLE}]").as_str())
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut array = [[0u8; WOTS_SINGLE]; 256];

                // Iterate through array elements
                for (i, item) in array.iter_mut().enumerate() {
                    *item = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                }

                // Check for excess elements
                if seq.next_element::<[u8; WOTS_SINGLE]>()?.is_some() {
                    return Err(de::Error::invalid_length(257, &self));
                }

                Ok(Wots256PublicKey(array))
            }
        }

        deserializer.deserialize_seq(Wots256PublicKeyVisitor)
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
    fn json_serialization() {
        let key = Wots256PublicKey([[1u8; WOTS_SINGLE]; 256]);

        // Test JSON serialization
        let serialized = serde_json::to_string(&key).unwrap();
        let deserialized: Wots256PublicKey = serde_json::from_str(&serialized).unwrap();

        assert_eq!(key, deserialized);
    }

    #[test]
    fn bincode_serialization() {
        let key = Wots256PublicKey([[2u8; WOTS_SINGLE]; 256]);

        // Test bincode serialization
        let serialized = bincode::serialize(&key).unwrap();
        let deserialized: Wots256PublicKey = bincode::deserialize(&serialized).unwrap();

        assert_eq!(key, deserialized);
    }

    #[test]
    #[should_panic]
    fn deserialize_too_few_elements() {
        let json = "[[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]]"; // Only one array
        let _: Wots256PublicKey = serde_json::from_str(json).unwrap();
    }

    #[test]
    #[should_panic]
    fn deserialize_too_many_elements() {
        // Create JSON string with 257 arrays
        let mut json = String::from("[");
        for i in 0..257 {
            json.push_str("[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]");
            if i < 256 {
                json.push(',');
            }
        }
        json.push(']');

        let _: Wots256PublicKey = serde_json::from_str(&json).unwrap();
    }

    #[test]
    #[should_panic]
    fn deserialize_invalid_array_length() {
        // Create JSON with one array having wrong length (19 instead of 20)
        let mut json = String::from("[");
        for i in 0..256 {
            if i == 128 {
                json.push_str("[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]"); // 19 elements
            } else {
                json.push_str("[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]");
            }
            if i < 255 {
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
        fn proptest_serde_json_roundtrip(key: Wots256PublicKey) {
            let serialized = serde_json::to_string(&key).unwrap();
            let deserialized: Wots256PublicKey = serde_json::from_str(&serialized).unwrap();
            prop_assert_eq!(key, deserialized);
        }

        #[test]
        fn proptest_bincode_roundtrip(key: Wots256PublicKey) {
            let serialized = bincode::serialize(&key).unwrap();
            let deserialized: Wots256PublicKey = bincode::deserialize(&serialized).unwrap();
            prop_assert_eq!(key, deserialized);
        }

        #[test]
        fn proptest_deref_operations(key: Wots256PublicKey) {
            let mut key_copy = key;

            // Test Deref
            prop_assert_eq!(&key.0, &*key);

            // Test DerefMut
            (*key_copy)[0] = [42u8; WOTS_SINGLE];
            prop_assert_eq!(key_copy.0[0], [42u8; WOTS_SINGLE]);
        }

        #[test]
        fn proptest_new_constructor(bytes: [[u8; WOTS_SINGLE]; 256]) {
            let key = Wots256PublicKey::new(bytes);
            prop_assert_eq!(key.0, bytes);
        }

        #[test]
            fn proptest_to_flattened_bytes(key: Wots256PublicKey) {
                let flattened = key.to_flattened_bytes();

                // Verify the length is correct
                prop_assert_eq!(flattened.len(), WOTS_SINGLE * 256);

                // Verify each segment matches the original arrays
                for (i, original_array) in key.0.iter().enumerate() {
                    let segment = &flattened[i * WOTS_SINGLE..(i + 1) * WOTS_SINGLE];
                    prop_assert_eq!(segment, original_array.as_slice());
                }
            }
    }
}
