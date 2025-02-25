//! Data necessary to process a deposit.
//!
//! Primarily Winternitz One-Time Signature (WOTS).

use std::fmt;

#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serde::{
    de::{self, SeqAccess, Visitor},
    Deserialize, Deserializer, Serialize, Serializer,
};

use crate::{wots::WOTS_SINGLE, Wots160PublicKey, Wots256PublicKey};

// NOTE: Taken from <https://github.com/alpenlabs/BitVM/blob/f7fd93e9c9187100e7c9b7b68e07f9f7c568a843/bitvm/src/chunk/compile.rs>
//       and <https://github.com/alpenlabs/BitVM/blob/f7fd93e9c9187100e7c9b7b68e07f9f7c568a843/bitvm/src/groth16/g16.rs#L7-L10>

/// Number of 256-bit WOTS public keys for the verifier.
pub const N_VERIFIER_PUBLIC_INPUTS: usize = 1;

/// Number of 256-bit WOTS public keys for the field elements.
pub const N_VERIFIER_FQS: usize = 20;

/// Number of 160-bit WOTS public keys for the hashes.
pub const N_VERIFIER_HASHES: usize = 380;

/// Winternitz One-Time Signature (WOTS) public key.
// NOTE: Taken from <https://github.com/alpenlabs/BitVM/blob/f7fd93e9c9187100e7c9b7b68e07f9f7c568a843/bitvm/src/groth16/g16.rs#L17-L21>
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct WotsPublicKeys(
    [Wots256PublicKey; N_VERIFIER_PUBLIC_INPUTS],
    [Wots256PublicKey; N_VERIFIER_FQS],
    [Wots160PublicKey; N_VERIFIER_HASHES],
);

// Custom Serialization for WotsPublicKeys
impl Serialize for WotsPublicKeys {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            use serde::ser::SerializeSeq;
            let mut seq = serializer.serialize_seq(Some(
                N_VERIFIER_PUBLIC_INPUTS + N_VERIFIER_FQS + N_VERIFIER_HASHES,
            ))?;

            // Serialize each element individually
            for key in &self.0 {
                seq.serialize_element(key)?;
            }
            for key in &self.1 {
                seq.serialize_element(key)?;
            }
            for key in &self.2 {
                seq.serialize_element(key)?;
            }
            seq.end()
        } else {
            // For binary formats, use bytes
            let mut bytes = Vec::with_capacity(
                N_VERIFIER_PUBLIC_INPUTS * 256 * WOTS_SINGLE
                    + N_VERIFIER_FQS * 256 * WOTS_SINGLE
                    + N_VERIFIER_HASHES * 160 * WOTS_SINGLE,
            );

            // Add public_inputs bytes
            for key in &self.0 {
                for inner in &key.0 {
                    bytes.extend_from_slice(inner);
                }
            }

            // Add fqs bytes
            for key in &self.1 {
                for inner in &key.0 {
                    bytes.extend_from_slice(inner);
                }
            }

            // Add hashes bytes
            for key in &self.2 {
                for inner in &key.0 {
                    bytes.extend_from_slice(inner);
                }
            }

            serializer.serialize_bytes(&bytes)
        }
    }
}

// Custom Deserialize for WotsPublicKeys
impl<'de> Deserialize<'de> for WotsPublicKeys {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WotsPublicKeysVisitor;

        impl<'de> Visitor<'de> for WotsPublicKeysVisitor {
            type Value = WotsPublicKeys;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a WotsPublicKeys structure")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut public_inputs =
                    [Wots256PublicKey([[0; WOTS_SINGLE]; 256]); N_VERIFIER_PUBLIC_INPUTS];
                let mut fqs = [Wots256PublicKey([[0; WOTS_SINGLE]; 256]); N_VERIFIER_FQS];
                let mut hashes = [Wots160PublicKey([[0; WOTS_SINGLE]; 160]); N_VERIFIER_HASHES];

                // Deserialize public_inputs
                for (i, public_input) in public_inputs
                    .iter_mut()
                    .enumerate()
                    .take(N_VERIFIER_PUBLIC_INPUTS)
                {
                    *public_input = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                }

                // Deserialize fqs
                for (i, fq) in fqs.iter_mut().enumerate().take(N_VERIFIER_FQS) {
                    *fq = seq.next_element()?.ok_or_else(|| {
                        de::Error::invalid_length(i + N_VERIFIER_PUBLIC_INPUTS, &self)
                    })?;
                }

                // Deserialize hashes
                for (i, hash) in hashes.iter_mut().enumerate().take(N_VERIFIER_HASHES) {
                    *hash = seq.next_element()?.ok_or_else(|| {
                        de::Error::invalid_length(
                            i + N_VERIFIER_PUBLIC_INPUTS + N_VERIFIER_FQS,
                            &self,
                        )
                    })?;
                }

                Ok(WotsPublicKeys(public_inputs, fqs, hashes))
            }

            fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // Calculate expected sizes
                const PUBLIC_INPUTS_SIZE: usize = N_VERIFIER_PUBLIC_INPUTS * 256 * WOTS_SINGLE;
                const FQS_SIZE: usize = N_VERIFIER_FQS * 256 * WOTS_SINGLE;
                const HASHES_SIZE: usize = N_VERIFIER_HASHES * 160 * WOTS_SINGLE;
                const EXPECTED_SIZE: usize = PUBLIC_INPUTS_SIZE + FQS_SIZE + HASHES_SIZE;

                if bytes.len() != EXPECTED_SIZE {
                    return Err(E::invalid_length(bytes.len(), &self));
                }

                // Split bytes into sections
                let (public_inputs_bytes, rest) = bytes.split_at(PUBLIC_INPUTS_SIZE);
                let (fqs_bytes, hashes_bytes) = rest.split_at(FQS_SIZE);

                // Convert bytes to arrays
                let mut public_inputs =
                    [Wots256PublicKey([[0; WOTS_SINGLE]; 256]); N_VERIFIER_PUBLIC_INPUTS];
                let mut fqs = [Wots256PublicKey([[0; WOTS_SINGLE]; 256]); N_VERIFIER_FQS];
                let mut hashes = [Wots160PublicKey([[0; WOTS_SINGLE]; 160]); N_VERIFIER_HASHES];

                // Fill public_inputs
                for (i, chunk) in public_inputs_bytes.chunks(256 * WOTS_SINGLE).enumerate() {
                    for (j, inner_chunk) in chunk.chunks(WOTS_SINGLE).enumerate() {
                        public_inputs[i].0[j].copy_from_slice(inner_chunk);
                    }
                }

                // Fill fqs
                for (i, chunk) in fqs_bytes.chunks(256 * WOTS_SINGLE).enumerate() {
                    for (j, inner_chunk) in chunk.chunks(WOTS_SINGLE).enumerate() {
                        fqs[i].0[j].copy_from_slice(inner_chunk);
                    }
                }

                // Fill hashes
                for (i, chunk) in hashes_bytes.chunks(160 * WOTS_SINGLE).enumerate() {
                    for (j, inner_chunk) in chunk.chunks(WOTS_SINGLE).enumerate() {
                        hashes[i].0[j].copy_from_slice(inner_chunk);
                    }
                }

                Ok(WotsPublicKeys(public_inputs, fqs, hashes))
            }
        }

        if deserializer.is_human_readable() {
            deserializer.deserialize_seq(WotsPublicKeysVisitor)
        } else {
            deserializer.deserialize_bytes(WotsPublicKeysVisitor)
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
        assert_eq!(N_VERIFIER_PUBLIC_INPUTS, 1);
        assert_eq!(N_VERIFIER_FQS, 20);
        assert_eq!(N_VERIFIER_HASHES, 380);
    }

    #[test]
    fn test_json_serialization() {
        let test_data = WotsPublicKeys(
            [Wots256PublicKey([[1u8; WOTS_SINGLE]; 256]); N_VERIFIER_PUBLIC_INPUTS],
            [Wots256PublicKey([[2u8; WOTS_SINGLE]; 256]); N_VERIFIER_FQS],
            [Wots160PublicKey([[3u8; WOTS_SINGLE]; 160]); N_VERIFIER_HASHES],
        );

        let serialized = serde_json::to_string(&test_data).unwrap();
        let deserialized: WotsPublicKeys = serde_json::from_str(&serialized).unwrap();

        assert_eq!(test_data.0[0], deserialized.0[0]);
        assert_eq!(test_data.1[0], deserialized.1[0]);
        assert_eq!(test_data.2[0], deserialized.2[0]);
    }

    #[test]
    fn test_bincode_serialization() {
        let test_data = WotsPublicKeys(
            [Wots256PublicKey([[1u8; WOTS_SINGLE]; 256]); N_VERIFIER_PUBLIC_INPUTS],
            [Wots256PublicKey([[2u8; WOTS_SINGLE]; 256]); N_VERIFIER_FQS],
            [Wots160PublicKey([[3u8; WOTS_SINGLE]; 160]); N_VERIFIER_HASHES],
        );

        let serialized = bincode::serialize(&test_data).unwrap();
        let deserialized: WotsPublicKeys = bincode::deserialize(&serialized).unwrap();

        assert_eq!(test_data.0[0], deserialized.0[0]);
        assert_eq!(test_data.1[0], deserialized.1[0]);
        assert_eq!(test_data.2[0], deserialized.2[0]);
    }

    #[test]
    #[should_panic]
    fn test_deserialize_invalid_length() {
        // Create a JSON array with wrong number of elements
        let mut json = String::from("[");
        for _ in 0..(N_VERIFIER_PUBLIC_INPUTS + N_VERIFIER_FQS + N_VERIFIER_HASHES - 1) {
            json.push_str(&format!(
                "{{\"array\":[[{}; {}]; {}]}}",
                1, WOTS_SINGLE, 256
            ));
            json.push(',');
        }
        json.push(']');

        let _: WotsPublicKeys = serde_json::from_str(&json).unwrap();
    }

    #[test]
    #[should_panic]
    fn test_deserialize_invalid_binary_length() {
        // Create invalid length binary data
        let invalid_data = vec![0u8; 100]; // Too short
        let _: WotsPublicKeys = bincode::deserialize(&invalid_data).unwrap();
    }

    #[cfg(feature = "proptest")]
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(50))]

        #[test]
        fn proptest_serde_json_roundtrip(byte: u8) {
            let test_data = WotsPublicKeys(
                [Wots256PublicKey([[byte; WOTS_SINGLE]; 256]); N_VERIFIER_PUBLIC_INPUTS],
                [Wots256PublicKey([[byte; WOTS_SINGLE]; 256]); N_VERIFIER_FQS],
                [Wots160PublicKey([[byte; WOTS_SINGLE]; 160]); N_VERIFIER_HASHES]
            );

            let serialized = serde_json::to_string(&test_data).unwrap();
            let deserialized: WotsPublicKeys = serde_json::from_str(&serialized).unwrap();

            prop_assert_eq!(test_data.0[0].0[0], deserialized.0[0].0[0]);
            prop_assert_eq!(test_data.1[0].0[0], deserialized.1[0].0[0]);
            prop_assert_eq!(test_data.2[0].0[0], deserialized.2[0].0[0]);
        }

        #[test]
        fn proptest_bincode_roundtrip(byte: u8) {
            let test_data = WotsPublicKeys(
                [Wots256PublicKey([[byte; WOTS_SINGLE]; 256]); N_VERIFIER_PUBLIC_INPUTS],
                [Wots256PublicKey([[byte; WOTS_SINGLE]; 256]); N_VERIFIER_FQS],
                [Wots160PublicKey([[byte; WOTS_SINGLE]; 160]); N_VERIFIER_HASHES]
            );

            let serialized = bincode::serialize(&test_data).unwrap();
            let deserialized: WotsPublicKeys = bincode::deserialize(&serialized).unwrap();

            prop_assert_eq!(test_data.0[0].0[0], deserialized.0[0].0[0]);
            prop_assert_eq!(test_data.1[0].0[0], deserialized.1[0].0[0]);
            prop_assert_eq!(test_data.2[0].0[0], deserialized.2[0].0[0]);
        }

        #[test]
        fn proptest_array_sizes(byte: u8) {
            let test_data = WotsPublicKeys(
                [Wots256PublicKey([[byte; WOTS_SINGLE]; 256]); N_VERIFIER_PUBLIC_INPUTS],
                [Wots256PublicKey([[byte; WOTS_SINGLE]; 256]); N_VERIFIER_FQS],
                [Wots160PublicKey([[byte; WOTS_SINGLE]; 160]); N_VERIFIER_HASHES]
            );

            prop_assert_eq!(test_data.0.len(), N_VERIFIER_PUBLIC_INPUTS);
            prop_assert_eq!(test_data.1.len(), N_VERIFIER_FQS);
            prop_assert_eq!(test_data.2.len(), N_VERIFIER_HASHES);
        }
    }

    #[test]
    fn test_full_structure() {
        let test_bytes = 0xFFu8;
        let test_data = WotsPublicKeys(
            [Wots256PublicKey([[test_bytes; WOTS_SINGLE]; 256]); N_VERIFIER_PUBLIC_INPUTS],
            [Wots256PublicKey([[test_bytes; WOTS_SINGLE]; 256]); N_VERIFIER_FQS],
            [Wots160PublicKey([[test_bytes; WOTS_SINGLE]; 160]); N_VERIFIER_HASHES],
        );

        // Test serialization/deserialization
        let serialized = bincode::serialize(&test_data).unwrap();
        let deserialized: WotsPublicKeys = bincode::deserialize(&serialized).unwrap();

        assert_eq!(test_data.0[0].0[0], deserialized.0[0].0[0]);
        assert_eq!(test_data.1[0].0[0], deserialized.1[0].0[0]);
        assert_eq!(test_data.2[0].0[0], deserialized.2[0].0[0]);

        // Test JSON serialization/deserialization
        let json_serialized = serde_json::to_string(&test_data).unwrap();
        let json_deserialized: WotsPublicKeys = serde_json::from_str(&json_serialized).unwrap();

        assert_eq!(test_data.0[0].0[0], json_deserialized.0[0].0[0]);
        assert_eq!(test_data.1[0].0[0], json_deserialized.1[0].0[0]);
        assert_eq!(test_data.2[0].0[0], json_deserialized.2[0].0[0]);
    }
}
