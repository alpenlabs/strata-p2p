use std::fmt;
use hex::ToHex;
use libp2p_identity::secp256k1::PublicKey;

/// OperatorPubKey serves as an identifier of protocol entity.
/// De facto this is a wrapper over `secp256k1::PublicKey`.
#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd,
)]
pub struct OperatorPubKey(pub Vec<u8>);

impl fmt::Display for OperatorPubKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.encode_hex::<String>())
    }
}

impl From<Vec<u8>> for OperatorPubKey {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<PublicKey> for OperatorPubKey {
    fn from(value: PublicKey) -> Self {
        Self(value.to_bytes().to_vec())
    }
}

impl OperatorPubKey {
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        match PublicKey::try_from_bytes(&self.0) {
            Ok(key) => key.verify(msg, sig),
            Err(_) => false,
        }
    }
}
