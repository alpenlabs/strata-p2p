//! Application signer trait for P2P message signing.
//!
//! This module provides the ApplicationSigner trait that allows service
//! libraries to provide signing functionality for messages without requiring
//! strata-p2p to store private keys.

use std::{fmt::Debug, sync::Arc};

#[cfg(not(feature = "byos"))]
use libp2p::identity::Keypair;
use libp2p::identity::PublicKey;

/// Trait for signing setup messages with application private keys.
///
/// The implementation should use the keypair that corresponds to the provided `app_public_key`.
pub trait ApplicationSigner: Debug + Send + Sync + 'static {
    /// Signs the given message with the application private key that corresponds to the
    /// app_public_key.
    fn sign(&self, message: &[u8]) -> Result<[u8; 64], Box<dyn std::error::Error + Send + Sync>>;
}

/// Internal signer that uses the transport keypair for signing when BYOS is disabled.
///
/// This signer is used automatically when BYOS feature is disabled, allowing the P2P
/// layer to sign messages using the transport keypair directly.
#[cfg(not(feature = "byos"))]
#[derive(Debug, Clone)]
pub struct TransportKeypairSigner {
    keypair: Keypair,
}

#[cfg(not(feature = "byos"))]
impl TransportKeypairSigner {
    /// Creates a new TransportKeypairSigner with the given transport keypair.
    pub const fn new(keypair: Keypair) -> Self {
        Self { keypair }
    }
}

#[cfg(not(feature = "byos"))]
impl ApplicationSigner for TransportKeypairSigner {
    fn sign(&self, message: &[u8]) -> Result<[u8; 64], Box<dyn std::error::Error + Send + Sync>> {
        // When BYOS is disabled, we ignore the app_public_key parameter and always
        // sign with the transport keypair
        let signature = self.keypair.sign(message)?;
        // Convert Vec<u8> to [u8; 64] array
        let mut array = [0u8; 64];
        if signature.len() != 64 {
            return Err("Signature length is not 64 bytes".into());
        }
        array.copy_from_slice(&signature);
        Ok(array)
    }
}

impl<T: ApplicationSigner + ?Sized> ApplicationSigner for Arc<T> {
    fn sign(&self, message: &[u8]) -> Result<[u8; 64], Box<dyn std::error::Error + Send + Sync>> {
        let signature = self.as_ref().sign(message)?;
        // Convert Vec<u8> to [u8; 64] array
        let mut array = [0u8; 64];
        if signature.len() != 64 {
            return Err("Signature length is not 64 bytes".into());
        }
        array.copy_from_slice(&signature);
        Ok(array)
    }
}
