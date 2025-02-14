//! Data necessary for a stake transaction.

use bitcoin::{
    hashes::{sha256, Hash},
    OutPoint,
};
use serde::{Deserialize, Serialize};

use crate::Wots256PublicKey;

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
    pub fn to_flattened_bytes(&self) -> [u8; 5188] {
        let mut bytes = [0u8; 5188];
        bytes[0..5120].copy_from_slice(&self.withdrawal_fulfillment_pk.to_flattened_bytes());
        bytes[5120..5152].copy_from_slice(&self.hash.to_byte_array());
        bytes[5152..5184].copy_from_slice(&self.operator_funds.txid.to_byte_array());
        bytes[5184..5188].copy_from_slice(&self.operator_funds.vout.to_be_bytes());
        bytes
    }
}
