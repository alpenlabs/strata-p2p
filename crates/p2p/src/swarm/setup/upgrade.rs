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
    signer::ApplicationSigner,
    swarm::message::{Message, P2PMessage, SetupMessage},
};

/// Custom codec for SetupMessage with manual serialization
#[derive(Debug, Clone, Default)]
pub(crate) struct SetupMessageCodec;

impl Encoder for SetupMessageCodec {
    type Item = (SetupMessage, Vec<u8>); // (message, signature)
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let (mut message, signature) = item;
        message.signature = signature; // Set the signature
        let bytes = message.to_bytes_with_signature();
        dst.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        dst.extend_from_slice(&bytes);
        Ok(())
    }
}

impl Decoder for SetupMessageCodec {
    type Item = SetupMessage; // (message, signature)
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
            Ok(message) => Ok(Some(message)),
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
    type Output = (SetupMessage, bool); // (message, signature_valid)
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, stream: Stream, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let mut framed = Framed::new(stream, SetupMessageCodec);
            match StreamExt::next(&mut framed).await {
                Some(Ok(message)) => {
                    // Verify the signature
                    let signature_valid = message.verify_signature().unwrap_or(false);
                    Ok((message, signature_valid))
                }
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
            // Create a signed message using Message::setup_signed
            let (message, signature) = Message::setup_signed(
                &self.app_public_key,
                self.local_peer_id,
                self.remote_peer_id,
                &self.signer,
            )
            .map_err(|e| io::Error::other(format!("Failed to create signed message: {e}")))?;

            let Message::Setup(setup_message) = message;

            let mut framed = Framed::new(stream, SetupMessageCodec);
            framed.send((setup_message, signature)).await?;
            framed.close().await
        })
    }
}
