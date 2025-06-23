//! Operators need to exchange (authenticated) messages which are signed with P2P
//! [`P2POperatorPubKey`].

use std::fmt;

use hex::ToHex;
use libp2p_identity::{ed25519::PublicKey, PeerId, PublicKey as WrapperPublicKey};

/// P2P [`P2POperatorPubKey`] serves as an identifier of protocol entity.
///
/// De facto this is a wrapper over [`PublicKey`].
#[derive(
    serde::Serialize, serde::Deserialize, Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd,
)]
pub struct P2POperatorPubKey(#[serde(with = "hex::serde")] Vec<u8>);

impl fmt::Display for P2POperatorPubKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.encode_hex::<String>())
    }
}

impl AsRef<[u8]> for P2POperatorPubKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<Vec<u8>> for P2POperatorPubKey {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl From<P2POperatorPubKey> for Vec<u8> {
    fn from(value: P2POperatorPubKey) -> Self {
        value.0
    }
}

impl From<PublicKey> for P2POperatorPubKey {
    fn from(value: PublicKey) -> Self {
        Self(value.to_bytes().to_vec())
    }
}

impl P2POperatorPubKey {
    /// Verifies the `message` using the `signature` against this [`P2POperatorPubKey`].
    pub fn verify(&self, message: &[u8], signature: &[u8]) -> bool {
        match PublicKey::try_from_bytes(&self.0) {
            Ok(key) => key.verify(message, signature),
            Err(_) => false,
        }
    }

    /// Returns the [`PeerId`] with respect to PublicKey.
    pub fn peer_id(&self) -> PeerId {
        // convert P2POperatorPubKey into LibP2P ed25519 PK
        let pk = PublicKey::try_from_bytes(&self.0).expect("infallible");
        let pk: WrapperPublicKey = pk.into();
        pk.into()
    }
}

