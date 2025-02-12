//! Operators need to exchange (authenticated) messages which are signed with [`OperatorPubKey`].

use std::fmt;

use hex::ToHex;
use libp2p_identity::secp256k1::PublicKey;

/// [`OperatorPubKey`] serves as an identifier of protocol entity.
///
/// De facto this is a wrapper over [`bitcoin::secp256k1::PublicKey`].
#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd,
)]
pub struct OperatorPubKey(Vec<u8>);

impl fmt::Display for OperatorPubKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.encode_hex::<String>())
    }
}

impl AsRef<[u8]> for OperatorPubKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for OperatorPubKey {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<OperatorPubKey> for Vec<u8> {
    fn from(value: OperatorPubKey) -> Self {
        value.0
    }
}

impl From<PublicKey> for OperatorPubKey {
    fn from(value: PublicKey) -> Self {
        Self(value.to_bytes().to_vec())
    }
}

impl OperatorPubKey {
    /// Verifies the `message` using the `signature` against this [`OperatorPubKey`].
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        match PublicKey::try_from_bytes(&self.0) {
            Ok(key) => key.verify(message, signature),
            Err(_) => false,
        }
    }
}
