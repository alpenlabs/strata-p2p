//! Common traits and signed message implementation.

use libp2p::identity::PublicKey;
use serde::{Deserialize, Serialize};

use crate::{
    signer::ApplicationSigner, swarm::serializing::signature_serialization::signature_serializer,
};

/// Trait for messages that can provide their application public key.
pub trait HasAppPublicKey {
    /// Get the application public key from the message.
    fn app_public_key(&self) -> &PublicKey;
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
    M: Serialize + HasAppPublicKey,
{
    /// Create new [`SignedMessage`] from message and sign via signer that implements
    /// [`ApplicationSigner`]
    pub fn new<S: ApplicationSigner>(
        message: M,
        signer: &S,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let signature = signer.sign(
            &serde_json::to_vec(&message)?,
            message.app_public_key().clone(),
        )?;
        Ok(Self { message, signature })
    }
}

impl<M> SignedMessage<M>
where
    M: Serialize + HasAppPublicKey,
{
    /// Verify the signature of the message.
    pub fn verify(&self) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        let serialized = serde_json::to_vec(&self.message)?;
        Ok(self
            .message
            .app_public_key()
            .verify(&serialized, &self.signature))
    }
}
