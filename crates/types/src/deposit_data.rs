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

/// Total size of the WOTS public keys in bytes.
pub const WOTS_PKS_TOTAL_SIZE: usize =
    (N_VERIFIER_PUBLIC_INPUTS * WOTS_SINGLE * Wots256PublicKey::SIZE)
        + (N_VERIFIER_FQS * WOTS_SINGLE * Wots256PublicKey::SIZE)
        + (N_VERIFIER_HASHES * WOTS_SINGLE * Wots160PublicKey::SIZE);

/// Winternitz One-Time Signature (WOTS) public key.
// NOTE: Taken from <https://github.com/alpenlabs/BitVM/blob/f7fd93e9c9187100e7c9b7b68e07f9f7c568a843/bitvm/src/groth16/g16.rs#L17-L21>
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct WotsPublicKeys {
    pub public_inputs: [Wots256PublicKey; N_VERIFIER_PUBLIC_INPUTS],
    pub fqs: [Wots256PublicKey; N_VERIFIER_FQS],
    pub hashes: [Wots160PublicKey; N_VERIFIER_HASHES],
}

impl WotsPublicKeys {
    /// Creates a new [`WotsPublicKeys`] instance.
    pub fn new(
        public_inputs: [Wots256PublicKey; N_VERIFIER_PUBLIC_INPUTS],
        fqs: [Wots256PublicKey; N_VERIFIER_FQS],
        hashes: [Wots160PublicKey; N_VERIFIER_HASHES],
    ) -> Self {
        Self {
            public_inputs,
            fqs,
            hashes,
        }
    }

    /// Length of the flattened byte array.
    pub const SIZE: usize = WOTS_PKS_TOTAL_SIZE;

    /// Converts [`WotsPublicKeys`] to a flattened byte array.
    pub fn to_flattened_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        let mut offset = 0;

        // Copy public_inputs bytes
        for public_input in &self.public_inputs {
            let flattened = public_input.to_flattened_bytes();
            bytes[offset..offset + WOTS_SINGLE * Wots256PublicKey::SIZE]
                .copy_from_slice(&flattened);
            offset += WOTS_SINGLE * Wots256PublicKey::SIZE;
        }

        // Copy fqs bytes
        for fq in &self.fqs {
            let flattened = fq.to_flattened_bytes();
            bytes[offset..offset + WOTS_SINGLE * Wots256PublicKey::SIZE]
                .copy_from_slice(&flattened);
            offset += WOTS_SINGLE * Wots256PublicKey::SIZE;
        }

        // Copy hashes bytes
        for hash in &self.hashes {
            let flattened = hash.to_flattened_bytes();
            bytes[offset..offset + WOTS_SINGLE * Wots160PublicKey::SIZE]
                .copy_from_slice(&flattened);
            offset += WOTS_SINGLE * Wots160PublicKey::SIZE;
        }

        bytes
    }

    /// Creates the [`Wots160PublicKey`] from a flattened byte array.
    ///
    /// If you already have structured nested arrays then you should use
    /// [`WotsPublicKeys::new`].
    ///
    /// # Panics
    ///
    /// Panics if the byte array is not of length `[u8; 1_323_520]`.
    pub fn from_flattened_bytes(bytes: &[u8]) -> Self {
        assert_eq!(
            bytes.len(),
            WOTS_PKS_TOTAL_SIZE,
            "Invalid byte array length"
        );

        let mut offset = 0;

        // Read public_inputs
        let mut public_inputs = [Wots256PublicKey([[0; WOTS_SINGLE]; Wots256PublicKey::SIZE]);
            N_VERIFIER_PUBLIC_INPUTS];
        for public_input in &mut public_inputs {
            let slice = &bytes[offset..offset + WOTS_SINGLE * Wots256PublicKey::SIZE];
            *public_input = Wots256PublicKey::from_flattened_bytes(slice);
            offset += WOTS_SINGLE * Wots256PublicKey::SIZE;
        }

        // Read fqs
        let mut fqs =
            [Wots256PublicKey([[0; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_FQS];
        for fq in &mut fqs {
            let slice = &bytes[offset..offset + WOTS_SINGLE * Wots256PublicKey::SIZE];
            *fq = Wots256PublicKey::from_flattened_bytes(slice);
            offset += WOTS_SINGLE * Wots256PublicKey::SIZE;
        }

        // Read hashes
        let mut hashes =
            [Wots160PublicKey([[0; WOTS_SINGLE]; Wots160PublicKey::SIZE]); N_VERIFIER_HASHES];
        for hash in &mut hashes {
            let slice = &bytes[offset..offset + WOTS_SINGLE * Wots160PublicKey::SIZE];
            *hash = Wots160PublicKey::from_flattened_bytes(slice);
            offset += WOTS_SINGLE * Wots160PublicKey::SIZE;
        }

        Self::new(public_inputs, fqs, hashes)
    }
}

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
            for key in &self.public_inputs {
                seq.serialize_element(key)?;
            }
            for key in &self.fqs {
                seq.serialize_element(key)?;
            }
            for key in &self.hashes {
                seq.serialize_element(key)?;
            }
            seq.end()
        } else {
            // For binary formats, use bytes
            let mut bytes = Vec::with_capacity(
                N_VERIFIER_PUBLIC_INPUTS * Wots256PublicKey::SIZE * WOTS_SINGLE
                    + N_VERIFIER_FQS * Wots256PublicKey::SIZE * WOTS_SINGLE
                    + N_VERIFIER_HASHES * Wots160PublicKey::SIZE * WOTS_SINGLE,
            );

            // Add public_inputs bytes
            for key in &self.public_inputs {
                for inner in &key.0 {
                    bytes.extend_from_slice(inner);
                }
            }

            // Add fqs bytes
            for key in &self.fqs {
                for inner in &key.0 {
                    bytes.extend_from_slice(inner);
                }
            }

            // Add hashes bytes
            for key in &self.hashes {
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
                    [Wots256PublicKey([[0; WOTS_SINGLE]; Wots256PublicKey::SIZE]);
                        N_VERIFIER_PUBLIC_INPUTS];
                let mut fqs =
                    [Wots256PublicKey([[0; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_FQS];
                let mut hashes = [Wots160PublicKey([[0; WOTS_SINGLE]; Wots160PublicKey::SIZE]);
                    N_VERIFIER_HASHES];

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

                Ok(WotsPublicKeys::new(public_inputs, fqs, hashes))
            }

            fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // Calculate expected sizes
                const PUBLIC_INPUTS_SIZE: usize =
                    N_VERIFIER_PUBLIC_INPUTS * Wots256PublicKey::SIZE * WOTS_SINGLE;
                const FQS_SIZE: usize = N_VERIFIER_FQS * Wots256PublicKey::SIZE * WOTS_SINGLE;
                const HASHES_SIZE: usize = N_VERIFIER_HASHES * Wots160PublicKey::SIZE * WOTS_SINGLE;
                const EXPECTED_SIZE: usize = PUBLIC_INPUTS_SIZE + FQS_SIZE + HASHES_SIZE;

                if bytes.len() != EXPECTED_SIZE {
                    return Err(E::invalid_length(bytes.len(), &self));
                }

                // Split bytes into sections
                let (public_inputs_bytes, rest) = bytes.split_at(PUBLIC_INPUTS_SIZE);
                let (fqs_bytes, hashes_bytes) = rest.split_at(FQS_SIZE);

                // Convert bytes to arrays
                let mut public_inputs =
                    [Wots256PublicKey([[0; WOTS_SINGLE]; Wots256PublicKey::SIZE]);
                        N_VERIFIER_PUBLIC_INPUTS];
                let mut fqs =
                    [Wots256PublicKey([[0; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_FQS];
                let mut hashes = [Wots160PublicKey([[0; WOTS_SINGLE]; Wots160PublicKey::SIZE]);
                    N_VERIFIER_HASHES];

                // Fill public_inputs
                for (i, chunk) in public_inputs_bytes
                    .chunks(Wots256PublicKey::SIZE * WOTS_SINGLE)
                    .enumerate()
                {
                    for (j, inner_chunk) in chunk.chunks(WOTS_SINGLE).enumerate() {
                        public_inputs[i].0[j].copy_from_slice(inner_chunk);
                    }
                }

                // Fill fqs
                for (i, chunk) in fqs_bytes
                    .chunks(Wots256PublicKey::SIZE * WOTS_SINGLE)
                    .enumerate()
                {
                    for (j, inner_chunk) in chunk.chunks(WOTS_SINGLE).enumerate() {
                        fqs[i].0[j].copy_from_slice(inner_chunk);
                    }
                }

                // Fill hashes
                for (i, chunk) in hashes_bytes
                    .chunks(Wots160PublicKey::SIZE * WOTS_SINGLE)
                    .enumerate()
                {
                    for (j, inner_chunk) in chunk.chunks(WOTS_SINGLE).enumerate() {
                        hashes[i].0[j].copy_from_slice(inner_chunk);
                    }
                }

                Ok(WotsPublicKeys::new(public_inputs, fqs, hashes))
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
        assert_eq!(WOTS_PKS_TOTAL_SIZE, 1_323_520);
    }

    #[test]
    fn test_json_serialization() {
        let test_data = WotsPublicKeys::new(
            [Wots256PublicKey([[1u8; WOTS_SINGLE]; Wots256PublicKey::SIZE]);
                N_VERIFIER_PUBLIC_INPUTS],
            [Wots256PublicKey([[2u8; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_FQS],
            [Wots160PublicKey([[3u8; WOTS_SINGLE]; Wots160PublicKey::SIZE]); N_VERIFIER_HASHES],
        );

        let serialized = serde_json::to_string(&test_data).unwrap();
        let deserialized: WotsPublicKeys = serde_json::from_str(&serialized).unwrap();

        assert_eq!(test_data.public_inputs[0], deserialized.public_inputs[0]);
        assert_eq!(test_data.fqs[0], deserialized.fqs[0]);
        assert_eq!(test_data.hashes[0], deserialized.hashes[0]);
    }

    #[test]
    fn test_bincode_serialization() {
        let test_data = WotsPublicKeys::new(
            [Wots256PublicKey([[1u8; WOTS_SINGLE]; Wots256PublicKey::SIZE]);
                N_VERIFIER_PUBLIC_INPUTS],
            [Wots256PublicKey([[2u8; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_FQS],
            [Wots160PublicKey([[3u8; WOTS_SINGLE]; Wots160PublicKey::SIZE]); N_VERIFIER_HASHES],
        );

        let serialized = bincode::serialize(&test_data).unwrap();
        let deserialized: WotsPublicKeys = bincode::deserialize(&serialized).unwrap();

        assert_eq!(test_data.public_inputs[0], deserialized.public_inputs[0]);
        assert_eq!(test_data.fqs[0], deserialized.fqs[0]);
        assert_eq!(test_data.hashes[0], deserialized.hashes[0]);
    }

    #[test]
    #[should_panic]
    fn test_deserialize_invalid_length() {
        // Create a JSON array with wrong number of elements
        let mut json = String::from("[");
        for _ in 0..(N_VERIFIER_PUBLIC_INPUTS + N_VERIFIER_FQS + N_VERIFIER_HASHES - 1) {
            json.push_str(&format!(
                "{{\"array\":[[{}; {}]; {}]}}",
                1,
                WOTS_SINGLE,
                Wots256PublicKey::SIZE
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
            let test_data = WotsPublicKeys::new(
                [Wots256PublicKey([[byte; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_PUBLIC_INPUTS],
                [Wots256PublicKey([[byte; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_FQS],
                [Wots160PublicKey([[byte; WOTS_SINGLE]; Wots160PublicKey::SIZE]); N_VERIFIER_HASHES]
            );

            let serialized = serde_json::to_string(&test_data).unwrap();
            let deserialized: WotsPublicKeys = serde_json::from_str(&serialized).unwrap();

            prop_assert_eq!(test_data.public_inputs[0].0[0], deserialized.public_inputs[0].0[0]);
            prop_assert_eq!(test_data.fqs[0].0[0], deserialized.fqs[0].0[0]);
            prop_assert_eq!(test_data.hashes[0].0[0], deserialized.hashes[0].0[0]);
        }

        #[test]
        fn proptest_bincode_roundtrip(byte: u8) {
            let test_data = WotsPublicKeys::new(
                [Wots256PublicKey([[byte; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_PUBLIC_INPUTS],
                [Wots256PublicKey([[byte; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_FQS],
                [Wots160PublicKey([[byte; WOTS_SINGLE]; Wots160PublicKey::SIZE]); N_VERIFIER_HASHES]
            );

            let serialized = bincode::serialize(&test_data).unwrap();
            let deserialized: WotsPublicKeys = bincode::deserialize(&serialized).unwrap();

            prop_assert_eq!(test_data.public_inputs[0].0[0], deserialized.public_inputs[0].0[0]);
            prop_assert_eq!(test_data.fqs[0].0[0], deserialized.fqs[0].0[0]);
            prop_assert_eq!(test_data.hashes[0].0[0], deserialized.hashes[0].0[0]);
        }

        #[test]
        fn proptest_array_sizes(byte: u8) {
            let test_data = WotsPublicKeys::new(
                [Wots256PublicKey([[byte; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_PUBLIC_INPUTS],
                [Wots256PublicKey([[byte; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_FQS],
                [Wots160PublicKey([[byte; WOTS_SINGLE]; Wots160PublicKey::SIZE]); N_VERIFIER_HASHES]
            );

            prop_assert_eq!(test_data.public_inputs.len(), N_VERIFIER_PUBLIC_INPUTS);
            prop_assert_eq!(test_data.fqs.len(), N_VERIFIER_FQS);
            prop_assert_eq!(test_data.hashes.len(), N_VERIFIER_HASHES);
        }
    }

    #[test]
    fn test_full_structure() {
        let test_bytes = 0xFFu8;
        let test_data = WotsPublicKeys::new(
            [Wots256PublicKey([[test_bytes; WOTS_SINGLE]; Wots256PublicKey::SIZE]);
                N_VERIFIER_PUBLIC_INPUTS],
            [Wots256PublicKey([[test_bytes; WOTS_SINGLE]; Wots256PublicKey::SIZE]); N_VERIFIER_FQS],
            [Wots160PublicKey([[test_bytes; WOTS_SINGLE]; Wots160PublicKey::SIZE]);
                N_VERIFIER_HASHES],
        );

        // Test serialization/deserialization
        let serialized = bincode::serialize(&test_data).unwrap();
        let deserialized: WotsPublicKeys = bincode::deserialize(&serialized).unwrap();

        assert_eq!(
            test_data.public_inputs[0].0[0],
            deserialized.public_inputs[0].0[0]
        );
        assert_eq!(test_data.fqs[0].0[0], deserialized.fqs[0].0[0]);
        assert_eq!(test_data.hashes[0].0[0], deserialized.hashes[0].0[0]);

        // Test JSON serialization/deserialization
        let json_serialized = serde_json::to_string(&test_data).unwrap();
        let json_deserialized: WotsPublicKeys = serde_json::from_str(&json_serialized).unwrap();

        assert_eq!(
            test_data.public_inputs[0].0[0],
            json_deserialized.public_inputs[0].0[0]
        );
        assert_eq!(test_data.fqs[0].0[0], json_deserialized.fqs[0].0[0]);
        assert_eq!(test_data.hashes[0].0[0], json_deserialized.hashes[0].0[0]);
    }
}
