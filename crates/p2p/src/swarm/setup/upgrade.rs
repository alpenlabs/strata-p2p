//! Protocol upgrade implementations for the setup phase.
//!
//! This module provides the upgrade implementations for both inbound and outbound
//! setup protocols, handling the serialization and exchange of setup messages
//! over libp2p streams.

#![cfg(feature = "byos")]

use std::{future::Future, iter, pin::Pin, sync::Arc};

use asynchronous_codec::{Framed, JsonCodec};
use futures::{SinkExt, StreamExt};
use libp2p::{
    InboundUpgrade, OutboundUpgrade, PeerId, Stream, core::UpgradeInfo, identity::PublicKey,
};

use crate::{
    signer::ApplicationSigner,
    swarm::{errors::SetupUpgradeError, message::SignedMessage},
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
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once("/handshake/1.0.0")
    }
}

impl InboundUpgrade<Stream> for InboundSetupUpgrade {
    type Output = SignedMessage;
    type Error = SetupUpgradeError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_inbound(self, stream: Stream, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let mut framed = Framed::new(stream, JsonCodec::<SignedMessage, SignedMessage>::new());

            match StreamExt::next(&mut framed).await {
                Some(Ok(signed_message)) => Ok(signed_message),
                Some(Err(e)) => Err(SetupUpgradeError::JsonCodec(e.into())),
                None => Err(SetupUpgradeError::UnexpectedStreamClose),
            }
        })
    }
}

/// Outbound upgrade for initiating handshake requests.
///
/// This upgrade sends the setup messages to remote peers.
#[derive(Clone, Debug)]
pub struct OutboundSetupUpgrade<S> {
    app_public_key: PublicKey,
    local_transport_id: PeerId,
    remote_transport_id: PeerId,
    signer: S,
}

impl<S> OutboundSetupUpgrade<S> {
    pub(crate) const fn new(
        app_public_key: PublicKey,
        local_transport_id: PeerId,
        remote_transport_id: PeerId,
        signer: S,
    ) -> Self {
        Self {
            app_public_key,
            local_transport_id,
            remote_transport_id,
            signer,
        }
    }
}

impl<S> UpgradeInfo for OutboundSetupUpgrade<S> {
    type Info = &'static str;
    type InfoIter = iter::Once<Self::Info>;

    fn protocol_info(&self) -> Self::InfoIter {
        iter::once("/handshake/1.0.0")
    }
}

impl OutboundUpgrade<Stream> for OutboundSetupUpgrade<Arc<dyn ApplicationSigner>> {
    type Output = ();
    type Error = SetupUpgradeError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Output, Self::Error>> + Send>>;

    fn upgrade_outbound(self, stream: Stream, _: Self::Info) -> Self::Future {
        Box::pin(async move {
            let setup_message = SignedMessage::new_signed_setup(
                self.app_public_key.clone(),
                self.local_transport_id,
                self.remote_transport_id,
                self.signer.as_ref(),
            )
            .map_err(|e| SetupUpgradeError::SignedMessageCreation(e.into()))?;

            let mut framed = Framed::new(stream, JsonCodec::<SignedMessage, SignedMessage>::new());
            framed
                .send(setup_message)
                .await
                .map_err(|e| SetupUpgradeError::JsonCodec(e.into()))?;
            framed
                .close()
                .await
                .map_err(|e| SetupUpgradeError::JsonCodec(e.into()))?;
            Ok(())
        })
    }
}
