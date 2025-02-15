//! Data necessary for a stake transaction.

use bitcoin::{
    hashes::{sha256, Hash},
    OutPoint,
};
use serde::{Deserialize, Serialize};

use crate::{wots::WOTS_SINGLE, Wots256PublicKey};

/// Size of a [`sha256::Hash`] in bytes.
pub const HASH_SIZE: usize = 32;

/// Size of a [`Txid`](bitcoin::Txid) in bytes.
pub const TXID_SIZE: usize = 32;

/// Size of a vout in bytes.
pub const VOUT_SIZE: usize = 4;

/// Stake data for a single stake transaction.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Deserialize, Serialize)]
pub struct StakeData {
    /// WOTS public keys for a single stake transaction.
    pub withdrawal_fulfillment_pk: Wots256PublicKey,

    /// Hashes for a single stake transaction.
    pub hash: sha256::Hash,

    /// Operator's funds prevouts for a single stake transaction.
    ///
    /// There are to cover the dust outputs in each withdrawal request.
    /// Composed of `txid:vout` ([`OutPoint`]) as a flattened byte array.
    pub operator_funds: OutPoint,
}

impl StakeData {
    /// Creates a new [`StakeData`] instance.
    pub fn new(
        withdrawal_fulfillment_pk: Wots256PublicKey,
        hash: sha256::Hash,
        operator_funds: OutPoint,
    ) -> Self {
        Self {
            withdrawal_fulfillment_pk,
            hash,
            operator_funds,
        }
    }

    /// Converts a [`StakeData`] instance into a flattened byte array.
    ///
    /// The byte array is structured as follows:
    ///
    /// - 5,120 (20 * 256) bytes for the withdrawal fulfillment public key.
    /// - 32 bytes for the hash.
    /// - 36 (32 + 4) bytes for the operator funds
    ///
    /// Total is 5,188 bytes.
    ///
    /// # Implementation Details
    ///
    /// Note that we are using little-endian encoding for the vout in the operator funds'
    /// [`OutPoint`].
    pub fn to_flattened_bytes(
        &self,
    ) -> [u8; WOTS_SINGLE * 256 + HASH_SIZE + TXID_SIZE + VOUT_SIZE] {
        let mut bytes = [0u8; WOTS_SINGLE * 256 + HASH_SIZE + TXID_SIZE + VOUT_SIZE];
        bytes[0..WOTS_SINGLE * 256]
            .copy_from_slice(&self.withdrawal_fulfillment_pk.to_flattened_bytes());
        bytes[WOTS_SINGLE * 256..WOTS_SINGLE * 256 + HASH_SIZE]
            .copy_from_slice(&self.hash.to_byte_array());
        bytes[WOTS_SINGLE * 256 + HASH_SIZE..WOTS_SINGLE * 256 + HASH_SIZE + TXID_SIZE]
            .copy_from_slice(&self.operator_funds.txid.to_byte_array());
        bytes[WOTS_SINGLE * 256 + HASH_SIZE + TXID_SIZE
            ..WOTS_SINGLE * 256 + HASH_SIZE + TXID_SIZE + VOUT_SIZE]
            .copy_from_slice(&self.operator_funds.vout.to_le_bytes());
        bytes
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "proptest")]
    use bitcoin::Txid;
    #[cfg(feature = "proptest")]
    use proptest::prelude::*;

    use super::*;

    // Sanity tests for the constants
    #[test]
    fn sanity_check_constants() {
        assert_eq!(HASH_SIZE, 32);
        assert_eq!(TXID_SIZE, 32);
        assert_eq!(VOUT_SIZE, 4);
    }

    #[cfg(feature = "proptest")]
    fn arb_outpoint() -> impl Strategy<Value = OutPoint> {
        // Generate arbitrary bytes for txid and vout
        (proptest::array::uniform32(any::<u8>()), any::<u32>()).prop_map(|(txid_bytes, vout)| {
            OutPoint {
                txid: Txid::from_byte_array(txid_bytes),
                vout,
            }
        })
    }

    #[cfg(feature = "proptest")]
    fn arb_sha256_hash() -> impl Strategy<Value = sha256::Hash> {
        proptest::array::uniform32(any::<u8>()).prop_map(sha256::Hash::from_byte_array)
    }

    #[cfg(feature = "proptest")]
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1_000))]

        #[test]
        fn proptest_stake_data_to_flattened_bytes(
            withdrawal_pk: Wots256PublicKey,
            hash in arb_sha256_hash(),
            operator_funds in arb_outpoint()
        ) {
            let stake_data = StakeData::new(withdrawal_pk, hash, operator_funds);
            let flattened = stake_data.to_flattened_bytes();

            // Verify total length
            prop_assert_eq!(flattened.len(), WOTS_SINGLE * 256 + HASH_SIZE + TXID_SIZE + 4);

            // Verify withdrawal fulfillment public key bytes
            let pk_bytes = &flattened[0..WOTS_SINGLE * 256];
            prop_assert_eq!(pk_bytes, &withdrawal_pk.to_flattened_bytes());

            // Verify hash bytes
            let hash_bytes = &flattened[WOTS_SINGLE * 256..WOTS_SINGLE * 256 + HASH_SIZE];
            prop_assert_eq!(hash_bytes, &stake_data.hash.to_byte_array());

            // Verify operator funds bytes
            let txid_bytes = &flattened[WOTS_SINGLE * 256 + HASH_SIZE..WOTS_SINGLE * 256 + HASH_SIZE + TXID_SIZE];
            prop_assert_eq!(txid_bytes, &stake_data.operator_funds.txid.to_byte_array());

            let vout_bytes = &flattened[WOTS_SINGLE * 256 + HASH_SIZE + TXID_SIZE..WOTS_SINGLE * 256 + HASH_SIZE + TXID_SIZE + 4];
            prop_assert_eq!(vout_bytes, &stake_data.operator_funds.vout.to_le_bytes());
        }
    }
}
