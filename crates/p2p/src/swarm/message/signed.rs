//! Common traits and signed message implementation.

use flexbuffers;
use libp2p::identity::PublicKey;
use serde::{Deserialize, Serialize};

use crate::{signer::ApplicationSigner, swarm::message::serde::signature_serializer};

/// Trait for messages that can provide their application public key.
pub trait HasPublicKey {
    /// Get the application public key from the message.
    fn public_key(&self) -> &PublicKey;
}

/// Signed message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignedMessage<M> {
    /// Underlying message
    pub message: M,
    /// Signature of message by application keypair,
    /// that can be verified by application public key from message field.
    #[serde(with = "signature_serializer")]
    pub signature: [u8; 64],
}

impl<M> SignedMessage<M>
where
    M: Serialize + HasPublicKey,
{
    /// Create new [`SignedMessage`] from message and sign via signer that implements
    /// [`ApplicationSigner`]
    pub async fn new(
        message: M,
        signer: &dyn ApplicationSigner,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let signature = signer.sign(&flexbuffers::to_vec(&message)?).await?;
        Ok(Self { message, signature })
    }
}

impl<M> SignedMessage<M>
where
    M: Serialize + HasPublicKey,
{
    /// Verify the signature of the message.
    pub fn verify(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let serialized = flexbuffers::to_vec(&self.message)?;
        Ok(self
            .message
            .public_key()
            .verify(&serialized, &self.signature))
    }
}
