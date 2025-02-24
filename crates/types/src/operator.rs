//! Operators need to exchange (authenticated) messages which are signed with [`OperatorPubKey`].

use std::fmt;

use bitcoin::{PublicKey, XOnlyPublicKey};
use hex::ToHex;
use secp256k1::{schnorr::Signature, Message, PublicKey as SecpPublicKey, SECP256K1};

/// [`OperatorPubKey`] serves as an identifier of protocol entity.
///
/// De facto this is a wrapper over [`bitcoin::secp256k1::XOnlyPublicKey`].
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

impl From<XOnlyPublicKey> for OperatorPubKey {
    fn from(value: XOnlyPublicKey) -> Self {
        Self(value.serialize().to_vec())
    }
}

impl From<PublicKey> for OperatorPubKey {
    fn from(value: PublicKey) -> Self {
        let xonly_pk = &value.to_bytes()[1..];
        Self(xonly_pk.to_vec())
    }
}

impl From<SecpPublicKey> for OperatorPubKey {
    fn from(value: SecpPublicKey) -> Self {
        Self(value.serialize().to_vec())
    }
}

impl OperatorPubKey {
    /// Verifies the `message` using the `signature` against this [`OperatorPubKey`].
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        let message = match Message::from_digest_slice(message) {
            Ok(message) => message,
            Err(_) => return false,
        };
        let signature = match Signature::from_slice(signature) {
            Ok(signature) => signature,
            Err(_) => return false,
        };
        match XOnlyPublicKey::from_slice(&self.0) {
            Ok(key) => key.verify(SECP256K1, &message, &signature).is_ok(),
            Err(_) => false,
        }
    }
}
