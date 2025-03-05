//! Data necessary to process a deposit.
//!
//! Primarily Winternitz One-Time Signature (WOTS).

#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::{Wots160PublicKey, Wots256PublicKey, WOTS_SINGLE};

/// Winternitz One-Time Signature (WOTS) public key.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct WotsPublicKeys {
    /// Number of public inputs.
    pub n_public_inputs: u8,

    /// Public inputs.
    pub public_inputs: Vec<Wots256PublicKey>,

    /// Number of field elements.
    pub n_field_elements: u8,

    /// Field Elements.
    pub fqs: Vec<Wots256PublicKey>,

    /// Number of hashes.
    pub n_hashes: u8,

    /// Hashes.
    pub hashes: Vec<Wots160PublicKey>,
}

impl WotsPublicKeys {
    /// Creates a new [`WotsPublicKeys`] instance.
    ///
    /// Note that you can create [`WotsPublicKeys`] that contains no public inputs, field elements,
    /// or hashes. For example:
    ///
    /// ```
    /// # use strata_p2p::types::WotsPublicKeys;
    /// let empty_wots = WotsPublicKeys::new(vec![], vec![], vec![]);
    /// # assert!(empty_wots.is_empty());
    ///
    /// let public_inputs = Wots256PublicKey::new([[1u8; 20]; 68]);
    /// let just_public_inputs = WotsPublicKeys::new(public_inputs, vec![], vec![]);
    ///
    /// let field_elements = Wots256PublicKey::new([[2u8; 20]; 68]);
    /// let just_field_elements = WotsPublicKeys::new(vec![], field_elements, vec![]);
    ///
    /// let hashes = Wots160PublicKey::new([[3u8; 20]; 44]);
    /// let just_hashes = WotsPublicKeys::new(vec![], vec![], hashes);
    /// ```
    pub fn new(
        public_inputs: Vec<Wots256PublicKey>,
        fqs: Vec<Wots256PublicKey>,
        hashes: Vec<Wots160PublicKey>,
    ) -> Self {
        Self {
            n_public_inputs: public_inputs.len() as u8,
            public_inputs,
            n_field_elements: fqs.len() as u8,
            fqs,
            n_hashes: hashes.len() as u8,
            hashes,
        }
    }

    /// Length of [`WotsPublicKeys`]
    pub fn len(&self) -> usize {
        (self.n_public_inputs + self.n_field_elements + self.n_hashes) as usize
    }

    /// If the collection is empty.
    pub fn is_empty(&self) -> bool {
        self.n_public_inputs == 0 && self.n_field_elements == 0 && self.n_hashes == 0
    }

    /// Converts [`WotsPublicKeys`] to a flattened byte array.
    ///
    /// # Format
    ///
    /// The flattened byte array is structured as follows:
    ///
    /// - The first byte represents the number of public inputs.
    /// - The second byte represents the number of field elements.
    /// - The third byte represents the number of hashes.
    /// - The next `self.n_public_inputs * 256 * 20` bytes represent the flattened bytes of each
    ///   public input.
    /// - The next `self.n_field_elements * 256 * 20` bytes represent the flattened bytes of each
    ///   field element.
    /// - The next `self.n_hashes * 160 * 20` bytes represent the flattened bytes of each hash.
    pub fn to_flattened_bytes(&self) -> Vec<u8> {
        // space for number of public_inputs, field_elements, and hashes as well as the length of
        // each
        let mut bytes = vec![];

        // Copy the number of public inputs, field elements, and hashes
        bytes.push(self.n_public_inputs);
        bytes.push(self.n_field_elements);
        bytes.push(self.n_hashes);

        // Copy public_inputs bytes
        for public_input in &self.public_inputs {
            let flattened = public_input.to_flattened_bytes();
            bytes.extend(flattened);
        }

        // Copy fqs bytes
        for fq in &self.fqs {
            let flattened = fq.to_flattened_bytes();
            bytes.extend(flattened);
        }

        // Copy hashes bytes
        for hash in &self.hashes {
            let flattened = hash.to_flattened_bytes();
            bytes.extend(flattened);
        }

        bytes
    }

    /// Creates the [`Wots160PublicKey`] from a flattened byte array.
    ///
    /// If you already have structured arrays then you should use [`WotsPublicKeys::new`].
    ///
    /// # Format
    ///
    /// The flattened byte array is structured as follows:
    ///
    /// - The first byte represents the number of public inputs.
    /// - The second byte represents the number of field elements.
    /// - The third byte represents the number of hashes.
    /// - The next `self.n_public_inputs * 256 * 20` bytes represent the flattened bytes of each
    ///   public input.
    /// - The next `self.n_field_elements * 256 * 20` bytes represent the flattened bytes of each
    ///   field element.
    /// - The next `self.n_hashes * 160 * 20` bytes represent the flattened bytes of each hash.
    pub fn from_flattened_bytes(bytes: &[u8]) -> Self {
        let mut offset = 0;

        // Read lengths
        let n_public_inputs = bytes[offset];
        offset += 1;
        let n_field_elements = bytes[offset];
        offset += 1;
        let n_hashes = bytes[offset];
        offset += 1;

        let mut public_inputs = Vec::with_capacity(n_public_inputs as usize);
        let mut fqs = Vec::with_capacity(n_field_elements as usize);
        let mut hashes = Vec::with_capacity(n_hashes as usize);

        // Read public_inputs
        for _ in 0..n_public_inputs {
            let slice = &bytes[offset..offset + WOTS_SINGLE * Wots256PublicKey::SIZE];
            public_inputs.push(Wots256PublicKey::from_flattened_bytes(slice));
            offset += WOTS_SINGLE * Wots256PublicKey::SIZE;
        }

        // Read field elements
        for _ in 0..n_field_elements {
            let slice = &bytes[offset..offset + WOTS_SINGLE * Wots256PublicKey::SIZE];
            fqs.push(Wots256PublicKey::from_flattened_bytes(slice));
            offset += WOTS_SINGLE * Wots256PublicKey::SIZE;
        }

        // Read hashes
        for _ in 0..n_hashes {
            let slice = &bytes[offset..offset + WOTS_SINGLE * Wots160PublicKey::SIZE];
            hashes.push(Wots160PublicKey::from_flattened_bytes(slice));
            offset += WOTS_SINGLE * Wots160PublicKey::SIZE;
        }

        Self {
            n_public_inputs,
            public_inputs,
            n_field_elements,
            fqs,
            n_hashes,
            hashes,
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "proptest")]
    use proptest::prelude::*;

    use super::*;
    use crate::wots::wots_total_digits;

    #[test]
    fn test_flattened_bytes_roundtrip() {
        // Create test data with known values
        let test_data = WotsPublicKeys::new(
            vec![Wots256PublicKey::new([[1u8; WOTS_SINGLE]; wots_total_digits(32)]); 2], /* 2 * 32 + 4 = 68 */
            vec![Wots256PublicKey::new([[2u8; WOTS_SINGLE]; wots_total_digits(32)]); 3], /* 2 * 32 + 4 = 68 */
            vec![Wots160PublicKey::new([[3u8; WOTS_SINGLE]; wots_total_digits(20)]); 4], /* 2 * 20 + 4 = 44 */
        );

        // Convert to flattened bytes
        let flattened = test_data.to_flattened_bytes();

        // Verify the length matches what we expect
        let expected_len = 3 + // 3 bytes for counts
            (2 * WOTS_SINGLE * Wots256PublicKey::SIZE) + // public inputs
            (3 * WOTS_SINGLE * Wots256PublicKey::SIZE) + // field elements
            (4 * WOTS_SINGLE * Wots160PublicKey::SIZE); // hashes
        assert_eq!(flattened.len(), expected_len);

        // Verify the counts are correct
        assert_eq!(flattened[0], 2); // n_public_inputs
        assert_eq!(flattened[1], 3); // n_field_elements
        assert_eq!(flattened[2], 4); // n_hashes

        // Convert back from flattened bytes
        let reconstructed = WotsPublicKeys::from_flattened_bytes(&flattened);

        // Verify all fields match
        assert_eq!(test_data.n_public_inputs, reconstructed.n_public_inputs);
        assert_eq!(test_data.n_field_elements, reconstructed.n_field_elements);
        assert_eq!(test_data.n_hashes, reconstructed.n_hashes);
        assert_eq!(test_data.public_inputs, reconstructed.public_inputs);
        assert_eq!(test_data.fqs, reconstructed.fqs);
        assert_eq!(test_data.hashes, reconstructed.hashes);
    }

    #[cfg(feature = "proptest")]
    proptest! {
        #[test]
        fn proptest_flattened_bytes_roundtrip(
            n_inputs in 0u8..5u8,
            n_fqs in 0u8..5u8,
            n_hashes in 0u8..5u8,
            value in 0u8..255u8
        ) {
            let test_data = WotsPublicKeys::new(
                vec![Wots256PublicKey::new([[value; WOTS_SINGLE]; Wots256PublicKey::SIZE]); n_inputs as usize],
                vec![Wots256PublicKey::new([[value; WOTS_SINGLE]; Wots256PublicKey::SIZE]); n_fqs as usize],
                vec![Wots160PublicKey::new([[value; WOTS_SINGLE]; Wots160PublicKey::SIZE]); n_hashes as usize],
            );

            let flattened = test_data.to_flattened_bytes();
            let reconstructed = WotsPublicKeys::from_flattened_bytes(&flattened);

            prop_assert_eq!(test_data.n_public_inputs, reconstructed.n_public_inputs);
            prop_assert_eq!(test_data.n_field_elements, reconstructed.n_field_elements);
            prop_assert_eq!(test_data.n_hashes, reconstructed.n_hashes);
            prop_assert_eq!(test_data.public_inputs, reconstructed.public_inputs);
            prop_assert_eq!(test_data.fqs, reconstructed.fqs);
            prop_assert_eq!(test_data.hashes, reconstructed.hashes);
        }
    }
}
