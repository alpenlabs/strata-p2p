use std::fmt;

use libp2p_identity::{PeerId, secp256k1::PublicKey};

#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd,
)]
pub struct OperatorPubkey(pub Vec<u8>);

impl fmt::Display for OperatorPubkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex_string = self.0.iter().fold(String::new(), |mut acc, byte| {
            acc.push_str(&format!("{:02x}", byte));
            acc
        });
        write!(f, "{}", hex_string)
    }
}

impl From<PeerId> for OperatorPubkey {
    fn from(value: PeerId) -> Self {
        Self(value.to_bytes())
    }
}

impl From<Vec<u8>> for OperatorPubkey {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<PublicKey> for OperatorPubkey {
    fn from(value: PublicKey) -> Self {
        Self(value.to_bytes().to_vec())
    }
}
