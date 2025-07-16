//! Protocol upgrade implementations for the setup handshake.
//!
//! This module provides the upgrade implementations for both inbound and outbound
//! handshake protocols, handling the serialization and exchange of handshake messages
//! over libp2p streams.

use std::{future::Future, pin::Pin};

use asynchronous_codec::{Decoder, Encoder, Framed};
use bytes::{Buf, BytesMut};
use futures::{SinkExt, StreamExt};
use libp2p::{
    InboundUpgrade, OutboundUpgrade, PeerId, Stream, core::UpgradeInfo, identity::PublicKey,
};
use tokio::io;

use crate::{
    message::{P2PMessage, SetupMessage},
    signer::ApplicationSigner,
};

/// Custom codec for SetupMessage with manual serialization
#[derive(Debug, Clone, Default)]
pub(crate) struct SetupMessageCodec;

impl Encoder for SetupMessageCodec {
    type Item = SetupMessage;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = item.to_bytes();
        dst.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        dst.extend_from_slice(&bytes);
        Ok(())
    }
}

impl Decoder for SetupMessageCodec {
    type Item = SetupMessage;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 4 {
            return Ok(None);
        }

        let len = u32::from_be_bytes([src[0], src[1], src[2], src[3]]) as usize;

        if src.len() < 4 + len {
            return Ok(None);
        }

        src.advance(4);
        let message_bytes = src.split_to(len);

        match SetupMessage::from_bytes(&message_bytes) {
            Ok((message, _)) => Ok(Some(message)),
            Err(e) => Err(e),
        }
    }
}

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
    type Output = SetupMessage;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, stream: Stream, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let mut framed = Framed::new(stream, SetupMessageCodec::default());
            match StreamExt::next(&mut framed).await {
                Some(Ok(message)) => Ok(message),
                Some(Err(e)) => Err(e),
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
    pub(super) fn new(
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
            // Create message
            let mut message = SetupMessage::new(
                &self.app_public_key,
                self.local_peer_id,
                self.remote_peer_id,
            );

            // Sign the message
            let content_to_sign = message.message_for_signing();

            // Get the app public key from the message to find the corresponding private key
            let app_public_key = message.get_app_public_key().map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("Failed to get app public key: {}", e),
                )
            })?;

            let signature = self.signer.sign(&content_to_sign, &app_public_key).map_err(|e| {
                io::Error::new(io::ErrorKind::Other, format!("Signing failed: {}", e))
            })?;

            message.set_signature(signature);

            let mut framed = Framed::new(stream, SetupMessageCodec::default());
            framed.send(message).await?;
            framed.close().await
        })
    }
}
