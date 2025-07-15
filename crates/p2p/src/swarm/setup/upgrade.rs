//! Protocol upgrade implementations for the setup handshake.
//!
//! This module provides the upgrade implementations for both inbound and outbound
//! handshake protocols, handling the serialization and exchange of handshake messages
//! over libp2p streams.

use std::{future::Future, pin::Pin};

use asynchronous_codec::{Framed, JsonCodec};
use futures::{SinkExt, StreamExt};
use libp2p::{InboundUpgrade, OutboundUpgrade, Stream, core::UpgradeInfo, identity::PublicKey};
use serde::{Deserialize, Serialize};
use tokio::io;

/// Message structure for the handshake protocol.
///
/// This message is exchanged between peers during the setup phase to
/// communicate application public keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeMessage {
    /// The application public key encoded as bytes
    pub(crate) app_public_key: Vec<u8>,
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
    type Output = HandshakeMessage;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, stream: Stream, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let mut framed = Framed::new(
                stream,
                JsonCodec::<HandshakeMessage, HandshakeMessage>::new(),
            );
            match StreamExt::next(&mut framed).await {
                Some(Ok(msg)) => Ok(msg),
                Some(Err(e)) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
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
#[derive(Debug, Clone)]
pub struct OutboundSetupUpgrade {
    pub(crate) app_public_key: PublicKey,
}

impl OutboundSetupUpgrade {
    pub(super) fn new(app_public_key: PublicKey) -> Self {
        Self { app_public_key }
    }
}

impl UpgradeInfo for OutboundSetupUpgrade {
    type Info = &'static str;
    type InfoIter = std::iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        std::iter::once("/handshake/1.0.0")
    }
}

impl OutboundUpgrade<Stream> for OutboundSetupUpgrade {
    type Output = ();
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, stream: Stream, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let app_message = HandshakeMessage {
                app_public_key: self.app_public_key.encode_protobuf(),
            };
            let mut framed = Framed::new(
                stream,
                JsonCodec::<HandshakeMessage, HandshakeMessage>::new(),
            );
            framed
                .send(app_message)
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            framed
                .close()
                .await
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
        })
    }
}
