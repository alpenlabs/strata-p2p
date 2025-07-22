//! Protocol upgrade implementations for the setup phase.
//!
//! This module provides the upgrade implementations for both inbound and outbound
//! setup protocols, handling the serialization and exchange of setup messages
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
    swarm::message::SignedMessage,
};

/// Inbound upgrade for handling incoming setup requests.
///
/// This upgrade processes incoming setup messages from remote peers.
#[derive(Debug, Clone)]
pub struct InboundSetupUpgrade;

impl InboundSetupUpgrade {
    /// Creates a new inbound setup upgrade.
    pub(crate) const fn new() -> Self {
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
    type Output = SignedMessage;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, stream: Stream, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let mut framed = Framed::new(stream, JsonCodec::<SignedMessage, SignedMessage>::new());

            match StreamExt::next(&mut framed).await {
                Some(Ok(signed_message)) => {
                    Ok(signed_message)
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
/// This upgrade sends the setup messages to remote peers.
#[derive(Clone, Debug)]
pub struct OutboundSetupUpgrade<S: ApplicationSigner> {
    pub(crate) app_public_key: PublicKey,
    pub(crate) local_peer_id: PeerId,
    pub(crate) remote_peer_id: PeerId,
    pub(crate) signer: S,
}

impl<S: ApplicationSigner> OutboundSetupUpgrade<S> {
    pub(crate) const fn new(
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
