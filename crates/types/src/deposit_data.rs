//! Data necessary to process a deposit.
//!
//! Primarily Winternitz One-Time Signature (WOTS).

#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serde::{Deserialize, Serialize};

use crate::{wots::Wots128PublicKey, Wots256PublicKey, WOTS_SINGLE};

/// Winternitz One-Time Signature (WOTS) public keys shared in a deposit.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct WotsPublicKeys {
    /// WOTS public key used for the Withdrawal Fulfillment transaction.
    pub withdrawal_fulfillment: Wots256PublicKey,

    /// WOTS public keys used for the Assert transaction in the Groth16 chunked proof.
    pub groth16: Groth16PublicKeys,
}

impl WotsPublicKeys {
    /// Creates a new [`WotsPublicKeys`] instance.
    ///
    /// # Examples
    ///
    /// ```
    /// # use strata_p2p_types::{WotsPublicKeys, Wots256PublicKey, Wots128PublicKey, Groth16PublicKeys};
    /// let withdrawal_key = Wots256PublicKey::new([[1u8; 20]; 68]);
    ///
    /// // Create a WotsPublicKeys with empty Groth16 parts
    /// let empty_groth16_keys = WotsPublicKeys::new(
    ///     withdrawal_key.clone(),
    ///     vec![],
    ///     vec![],
    ///     vec![]
    /// );
    ///
    /// // Create a WotsPublicKeys with some Groth16 parts
    /// let public_inputs = Wots256PublicKey::new([[2u8; 20]; 68]);
    /// let field_elements = Wots256PublicKey::new([[3u8; 20]; 68]);
    /// let hashes = Wots128PublicKey::new([[4u8; 20]; 36]);
    ///
    /// let wots_keys = WotsPublicKeys::new(
    ///     withdrawal_key,
    ///     vec![public_inputs],
    ///     vec![field_elements],
    ///     vec![hashes],
    /// );
    /// ```
    pub fn new(
        withdrawal_fulfillment: Wots256PublicKey,
        public_inputs: Vec<Wots256PublicKey>,
        fqs: Vec<Wots256PublicKey>,
        hashes: Vec<Wots128PublicKey>,
    ) -> Self {
        Self {
            withdrawal_fulfillment,
            groth16: Groth16PublicKeys::new(public_inputs, fqs, hashes),
        }
    }

    /// Creates a [`WotsPublicKeys`] from a flattened byte array.
    ///
    /// # Format
    ///
    /// The flattened byte array is structured as follows:
    ///
    /// - The first 3 bytes of the Groth16PublicKeys (containing counts)
    /// - The next `WOTS_SINGLE * Wots256PublicKey::SIZE` bytes represent the withdrawal_fulfillment
    ///   key
    /// - The remaining bytes represent the flattened Groth16PublicKeys as described in
    ///   [`Groth16PublicKeys::to_flattened_bytes`]
    pub fn from_flattened_bytes(bytes: &[u8]) -> Self {
        // The withdrawal fulfillment key size in flattened form
        let withdrawal_key_size = WOTS_SINGLE * Wots256PublicKey::SIZE;

        // Parse the withdrawal fulfillment key
        let withdrawal_fulfillment =
            Wots256PublicKey::from_flattened_bytes(&bytes[0..withdrawal_key_size]);

        // Parse the Groth16 public keys from the remaining bytes
        let groth16 = Groth16PublicKeys::from_flattened_bytes(&bytes[withdrawal_key_size..]);

        Self {
            withdrawal_fulfillment,
            groth16,
        }
    }

    /// Converts [`WotsPublicKeys`] to a flattened byte array.
    ///
    /// # Format
    ///
    /// The flattened byte array is structured as follows:
    ///
    /// - The first `WOTS_SINGLE * Wots256PublicKey::SIZE` bytes represent the
    ///   withdrawal_fulfillment key
    /// - The remaining bytes represent the flattened Groth16PublicKeys as described in
    ///   [`Groth16PublicKeys::to_flattened_bytes`]
    pub fn to_flattened_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();

        // Add withdrawal fulfillment key bytes
        bytes.extend(self.withdrawal_fulfillment.to_flattened_bytes());

        // Add Groth16 public keys bytes
        bytes.extend(self.groth16.to_flattened_bytes());

        bytes
    }
}

/// Winternitz One-Time Signature (WOTS) public keys used for the Assert transaction
/// in the Groth16 proof.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct Groth16PublicKeys {
    /// Number of public inputs.
    pub n_public_inputs: u8,

    /// Public inputs used when passing state in chunked Groth16 proofs.
    pub public_inputs: Vec<Wots256PublicKey>,

    /// Number of field elements.
    pub n_field_elements: u8,

    /// Field Elements used when passing state in chunked Groth16 proofs.
    pub fqs: Vec<Wots256PublicKey>,

    /// Number of hashes.
    pub n_hashes: u8,

    /// Hashes used when passing state in chunked Groth16 proofs.
    pub hashes: Vec<Wots128PublicKey>,
}

impl Groth16PublicKeys {
    /// Creates a new [`Groth16PublicKeys`] instance.
    ///
    /// Note that you can create [`Groth16PublicKeys`] that contains no public inputs, field
    /// elements, or hashes. For example:
    ///
    /// ```
    /// # use strata_p2p_types::{Groth16PublicKeys, Wots256PublicKey, Wots128PublicKey};
    /// let empty_wots = Groth16PublicKeys::new(vec![], vec![], vec![]);
    /// # assert!(empty_wots.is_empty());
    ///
    /// let public_inputs = Wots256PublicKey::new([[1u8; 20]; 68]);
    /// let just_public_inputs = Groth16PublicKeys::new(vec![public_inputs], vec![], vec![]);
    ///
    /// let field_elements = Wots256PublicKey::new([[2u8; 20]; 68]);
    /// let just_field_elements = Groth16PublicKeys::new(vec![], vec![field_elements], vec![]);
    ///
    /// let hashes = Wots128PublicKey::new([[3u8; 20]; 36]);
    /// let just_hashes = Groth16PublicKeys::new(vec![], vec![], vec![hashes]);
    /// ```
    pub fn new(
        public_inputs: Vec<Wots256PublicKey>,
        fqs: Vec<Wots256PublicKey>,
        hashes: Vec<Wots128PublicKey>,
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

    /// Converts [`Groth16PublicKeys`] to a flattened byte array.
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
    /// - The next `self.n_hashes * 128 * 20` bytes represent the flattened bytes of each hash.
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

    /// Creates the [`Groth16PublicKeys`] from a flattened byte array.
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
    /// - The next `self.n_hashes * 128 * 20` bytes represent the flattened bytes of each hash.
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
            let slice = &bytes[offset..offset + WOTS_SINGLE * Wots128PublicKey::SIZE];
            hashes.push(Wots128PublicKey::from_flattened_bytes(slice));
            offset += WOTS_SINGLE * Wots128PublicKey::SIZE;
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
    fn groth16_wots_flattened_bytes_roundtrip() {
        // Create test data with known values
        let test_data = Groth16PublicKeys::new(
            vec![Wots256PublicKey::new([[1u8; WOTS_SINGLE]; wots_total_digits(32)]); 2], /* 2 * 32 + 4 = 68 */
            vec![Wots256PublicKey::new([[2u8; WOTS_SINGLE]; wots_total_digits(32)]); 3], /* 2 * 32 + 4 = 68 */
            vec![Wots128PublicKey::new([[3u8; WOTS_SINGLE]; wots_total_digits(16)]); 4], /* 2 * 16 + 4 = 36 */
        );

        // Convert to flattened bytes
        let flattened = test_data.to_flattened_bytes();

        // Verify the length matches what we expect
        let expected_len = 3 + // 3 bytes for counts
            (2 * WOTS_SINGLE * Wots256PublicKey::SIZE) + // public inputs
            (3 * WOTS_SINGLE * Wots256PublicKey::SIZE) + // field elements
            (4 * WOTS_SINGLE * Wots128PublicKey::SIZE); // hashes
        assert_eq!(flattened.len(), expected_len);

        // Verify the counts are correct
        assert_eq!(flattened[0], 2); // n_public_inputs
        assert_eq!(flattened[1], 3); // n_field_elements
        assert_eq!(flattened[2], 4); // n_hashes

        // Convert back from flattened bytes
        let reconstructed = Groth16PublicKeys::from_flattened_bytes(&flattened);

        // Verify all fields match
        assert_eq!(test_data.n_public_inputs, reconstructed.n_public_inputs);
        assert_eq!(test_data.n_field_elements, reconstructed.n_field_elements);
        assert_eq!(test_data.n_hashes, reconstructed.n_hashes);
        assert_eq!(test_data.public_inputs, reconstructed.public_inputs);
        assert_eq!(test_data.fqs, reconstructed.fqs);
        assert_eq!(test_data.hashes, reconstructed.hashes);
    }

    #[test]
    fn wots_public_keys_flattened_bytes_roundtrip() {
        // Create withdrawal fulfillment key
        let withdrawal_fulfillment =
            Wots256PublicKey::new([[4u8; WOTS_SINGLE]; wots_total_digits(32)]);

        // Create Groth16 public keys
        let groth16 = Groth16PublicKeys::new(
            vec![Wots256PublicKey::new([[1u8; WOTS_SINGLE]; wots_total_digits(32)]); 2],
            vec![Wots256PublicKey::new([[2u8; WOTS_SINGLE]; wots_total_digits(32)]); 3],
            vec![Wots128PublicKey::new([[3u8; WOTS_SINGLE]; wots_total_digits(16)]); 4],
        );

        // Create the WotsPublicKeys
        let wots_keys = WotsPublicKeys::new(
            withdrawal_fulfillment,
            groth16.public_inputs.clone(),
            groth16.fqs.clone(),
            groth16.hashes.clone(),
        );

        // Convert to flattened bytes
        let flattened = wots_keys.to_flattened_bytes();

        // Verify the length matches what we expect
        let expected_len = (WOTS_SINGLE * Wots256PublicKey::SIZE) + // withdrawal_fulfillment
            3 + // 3 bytes for counts
            (2 * WOTS_SINGLE * Wots256PublicKey::SIZE) + // public inputs
            (3 * WOTS_SINGLE * Wots256PublicKey::SIZE) + // field elements
            (4 * WOTS_SINGLE * Wots128PublicKey::SIZE); // hashes
        assert_eq!(flattened.len(), expected_len);

        // Check that the first part is the withdrawal fulfillment key
        let withdrawal_bytes = withdrawal_fulfillment.to_flattened_bytes();
        assert_eq!(&flattened[0..withdrawal_bytes.len()], &withdrawal_bytes[..]);

        // Convert back from flattened bytes
        let reconstructed = WotsPublicKeys::from_flattened_bytes(&flattened);

        // Verify all fields match
        assert_eq!(
            wots_keys.withdrawal_fulfillment,
            reconstructed.withdrawal_fulfillment
        );
        assert_eq!(
            wots_keys.groth16.n_public_inputs,
            reconstructed.groth16.n_public_inputs
        );
        assert_eq!(
            wots_keys.groth16.n_field_elements,
            reconstructed.groth16.n_field_elements
        );
        assert_eq!(wots_keys.groth16.n_hashes, reconstructed.groth16.n_hashes);
        assert_eq!(
            wots_keys.groth16.public_inputs,
            reconstructed.groth16.public_inputs
        );
        assert_eq!(wots_keys.groth16.fqs, reconstructed.groth16.fqs);
        assert_eq!(wots_keys.groth16.hashes, reconstructed.groth16.hashes);

        // Full equality check
        assert_eq!(wots_keys, reconstructed);
    }

    #[cfg(feature = "proptest")]
    proptest! {
        #[test]
        fn proptest_groth16_wots_flattened_bytes_roundtrip(
            n_inputs in 0u8..5u8,
            n_fqs in 0u8..5u8,
            n_hashes in 0u8..5u8,
            value in 0u8..255u8
        ) {
            let test_data = Groth16PublicKeys::new(
                vec![Wots256PublicKey::new([[value; WOTS_SINGLE]; Wots256PublicKey::SIZE]); n_inputs as usize],
                vec![Wots256PublicKey::new([[value; WOTS_SINGLE]; Wots256PublicKey::SIZE]); n_fqs as usize],
                vec![Wots128PublicKey::new([[value; WOTS_SINGLE]; Wots128PublicKey::SIZE]); n_hashes as usize],
            );

            let flattened = test_data.to_flattened_bytes();
            let reconstructed = Groth16PublicKeys::from_flattened_bytes(&flattened);

            prop_assert_eq!(test_data.n_public_inputs, reconstructed.n_public_inputs);
            prop_assert_eq!(test_data.n_field_elements, reconstructed.n_field_elements);
            prop_assert_eq!(test_data.n_hashes, reconstructed.n_hashes);
            prop_assert_eq!(test_data.public_inputs, reconstructed.public_inputs);
            prop_assert_eq!(test_data.fqs, reconstructed.fqs);
            prop_assert_eq!(test_data.hashes, reconstructed.hashes);
        }

        #[test]
        fn proptest_wots_public_keys_flattened_bytes_roundtrip(
            withdrawal_value in 0u8..255u8,
            n_inputs in 0u8..3u8,
            n_fqs in 0u8..3u8,
            n_hashes in 0u8..3u8,
            groth16_value in 0u8..255u8
        ) {
            // Create test data with different values for withdrawal and groth16
            let withdrawal_fulfillment = Wots256PublicKey::new(
                [[withdrawal_value; WOTS_SINGLE]; Wots256PublicKey::SIZE]
            );

            let groth16 = Groth16PublicKeys::new(
                vec![Wots256PublicKey::new([[groth16_value; WOTS_SINGLE]; Wots256PublicKey::SIZE]); n_inputs as usize],
                vec![Wots256PublicKey::new([[groth16_value; WOTS_SINGLE]; Wots256PublicKey::SIZE]); n_fqs as usize],
                vec![Wots128PublicKey::new([[groth16_value; WOTS_SINGLE]; Wots128PublicKey::SIZE]); n_hashes as usize],
            );

            let wots_keys = WotsPublicKeys::new(
                withdrawal_fulfillment,
                groth16.public_inputs.clone(),
                groth16.fqs.clone(),
                groth16.hashes.clone(),
            );

            let flattened = wots_keys.to_flattened_bytes();
            let reconstructed = WotsPublicKeys::from_flattened_bytes(&flattened);

            prop_assert_eq!(wots_keys.withdrawal_fulfillment, reconstructed.withdrawal_fulfillment);
            prop_assert_eq!(wots_keys.groth16.n_public_inputs, reconstructed.groth16.n_public_inputs);
            prop_assert_eq!(wots_keys.groth16.n_field_elements, reconstructed.groth16.n_field_elements);
            prop_assert_eq!(wots_keys.groth16.n_hashes, reconstructed.groth16.n_hashes);
            prop_assert_eq!(wots_keys.groth16.public_inputs.clone(), reconstructed.groth16.public_inputs.clone());
            prop_assert_eq!(wots_keys.groth16.fqs.clone(), reconstructed.groth16.fqs.clone());
            prop_assert_eq!(wots_keys.groth16.hashes.clone(), reconstructed.groth16.hashes.clone());
            prop_assert_eq!(wots_keys, reconstructed);
        }
    }
}
