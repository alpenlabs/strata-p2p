//! Protocol upgrade implementations for the setup handshake.
//!
//! This module provides the upgrade implementations for both inbound and outbound
//! handshake protocols, handling the serialization and exchange of handshake messages
//! over libp2p streams.

use std::{future::Future, pin::Pin};

use asynchronous_codec::{Framed, JsonCodec};
use futures::{SinkExt, StreamExt};
use libp2p::{
    InboundUpgrade, OutboundUpgrade, PeerId, Stream, core::UpgradeInfo, identity::PublicKey,
};
use tokio::io;

use crate::{
    signer::ApplicationSigner,
    swarm::message::{SetupMessage, SignedMessage},
};

/// Inbound upgrade for handling incoming handshake requests.
///
/// This upgrade processes incoming handshake messages from remote peers,
/// deserializing their application public keys.
#[derive(Debug, Clone)]
pub struct InboundSetupUpgrade;

impl InboundSetupUpgrade {
    /// Creates a new inbound setup upgrade.
    pub(super) const fn new() -> Self {
        Self
    }
}

impl UpgradeInfo for InboundSetupUpgrade {
    type Info = &'static str;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once("/handshake/1.0.0")
    }
}

impl InboundUpgrade<Stream> for InboundSetupUpgrade {
    type Output = (SignedMessage, bool); // (signed_message, signature_valid)
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, stream: Stream, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let mut framed = Framed::new(stream, JsonCodec::<SignedMessage, SignedMessage>::new());

            match StreamExt::next(&mut framed).await {
                Some(Ok(signed_message)) => {
                    let setup_message: SetupMessage =
                        signed_message.deserialize_message().map_err(|e| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!("Failed to deserialize message: {e}"),
                            )
                        })?;
                    let app_public_key = setup_message.get_app_public_key().map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("Invalid app public key: {e}"),
                        )
                    })?;
                    let signature_valid = signed_message
                        .verify_signature(&app_public_key)
                        .unwrap_or(false);

                    Ok((signed_message, signature_valid))
                }
                Some(Err(e)) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("JSON decode error: {e}"),
                )),
                None => Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "stream closed",
                )),
            }
        })
    }
}

/// Outbound upgrade for initiating handshake requests.
///
/// This upgrade sends the local application public key to remote peers
/// as part of the handshake protocol.
#[derive(Clone, Debug)]
pub struct OutboundSetupUpgrade<S: ApplicationSigner> {
    pub(crate) app_public_key: PublicKey,
    pub(crate) local_peer_id: PeerId,
    pub(crate) remote_peer_id: PeerId,
    pub(crate) signer: S,
}

impl<S: ApplicationSigner> OutboundSetupUpgrade<S> {
    pub(super) const fn new(
        app_public_key: PublicKey,
        local_peer_id: PeerId,
        remote_peer_id: PeerId,
        signer: S,
    ) -> Self {
        Self {
            app_public_key,
            local_peer_id,
            remote_peer_id,
            signer,
        }
    }
}

impl<S: ApplicationSigner> UpgradeInfo for OutboundSetupUpgrade<S> {
    type Info = &'static str;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once("/handshake/1.0.0")
    }
}

impl<S: ApplicationSigner> OutboundUpgrade<Stream> for OutboundSetupUpgrade<S> {
    type Output = ();
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, stream: Stream, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            // Create a signed message directly
            let setup_message = SignedMessage::new_signed_setup(
                self.app_public_key.clone(),
                self.local_peer_id,
                self.remote_peer_id,
                &self.signer,
            )
            .map_err(|e| io::Error::other(format!("Failed to create signed message: {e}")))?;

            let mut framed = Framed::new(stream, JsonCodec::<SignedMessage, SignedMessage>::new());
            framed.send(setup_message).await.map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("JSON encode error: {e}"),
                )
            })?;
            framed.close().await.map_err(|e| {
                io::Error::new(io::ErrorKind::Other, format!("Stream close error: {e}"))
            })
        })
    }
}
