//! Application signer trait for P2P message signing.
//!
//! This module provides the ApplicationSigner trait that allows service
//! libraries to provide signing functionality for messages without requiring
//! strata-p2p to store private keys.

use std::fmt::Debug;

use libp2p::identity::PublicKey;

/// Trait for signing setup messages with application private keys.
///
/// The implementation should use the keypair that corresponds to the provided `app_public_key`.
pub trait ApplicationSigner: Debug + Send + Sync + 'static {
    /// Signs the given message with the application private key that corresponds to the
    /// app_public_key.
    fn sign(
        &self,
        message: &[u8],
        app_public_key: PublicKey,
    ) -> Result<[u8; 64], Box<dyn std::error::Error + Send + Sync>>;
}
