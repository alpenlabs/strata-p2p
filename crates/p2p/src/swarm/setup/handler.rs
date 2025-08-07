//! Connection handler for the setup protocol.
//!
//! This module provides the [`SetupHandler`] which manages individual connection
//! setup processes, handling both inbound and outbound substreams for the setup
//! protocol.

#![cfg(feature = "byos")]

use std::{
    sync::Arc,
    task::{Context, Poll},
};

use libp2p::{
    PeerId,
    identity::PublicKey,
    swarm::{
        ConnectionHandler, ConnectionHandlerEvent, SubstreamProtocol, handler::ConnectionEvent,
    },
};

use crate::{
    signer::ApplicationSigner,
    swarm::{
        errors::SetupError,
        setup::{
            events::SetupHandlerEvent,
            upgrade::{InboundSetupUpgrade, OutboundSetupUpgrade},
        },
    },
};

/// Connection handler for managing setup protocol substreams.
///
/// This handler manages the lifecycle of setup substreams for a single connection,
/// coordinating both inbound and outbound handshake processes.
#[derive(Debug)]
pub struct SetupHandler {
    outbound_substream:
        Option<SubstreamProtocol<OutboundSetupUpgrade<Arc<dyn ApplicationSigner>>, ()>>,
    pending_events: Vec<
        ConnectionHandlerEvent<
            OutboundSetupUpgrade<Arc<dyn ApplicationSigner>>,
            (),
            SetupHandlerEvent,
        >,
    >,
}

impl SetupHandler {
    pub(crate) fn new(
        app_public_key: PublicKey,
        local_transport_id: PeerId,
        remote_transport_id: PeerId,
        signer: Arc<dyn ApplicationSigner>,
    ) -> Self {
        let upgrade = OutboundSetupUpgrade::new(
            app_public_key,
            local_transport_id,
            remote_transport_id,
            signer,
        );

        Self {
            outbound_substream: Some(SubstreamProtocol::new(upgrade, ())),
            pending_events: Vec::new(),
        }
    }
}

impl ConnectionHandler for SetupHandler {
    type FromBehaviour = ();
    type ToBehaviour = SetupHandlerEvent;
    type InboundProtocol = InboundSetupUpgrade;
    type OutboundProtocol = OutboundSetupUpgrade<Arc<dyn ApplicationSigner>>;
    type InboundOpenInfo = ();
    type OutboundOpenInfo = ();

    fn listen_protocol(&self) -> SubstreamProtocol<Self::InboundProtocol, ()> {
        SubstreamProtocol::new(InboundSetupUpgrade::new(), ())
    }

    fn connection_keep_alive(&self) -> bool {
        true
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<ConnectionHandlerEvent<Self::OutboundProtocol, (), Self::ToBehaviour>> {
        if let Some(event) = self.pending_events.pop() {
            return Poll::Ready(event);
        }

        if let Some(substream) = self.outbound_substream.take() {
            return Poll::Ready(ConnectionHandlerEvent::OutboundSubstreamRequest {
                protocol: substream,
            });
        }

        Poll::Pending
    }

    fn on_behaviour_event(&mut self, _: Self::FromBehaviour) {}

    fn on_connection_event(
        &mut self,
        event: ConnectionEvent<'_, Self::InboundProtocol, Self::OutboundProtocol>,
    ) {
        match event {
            ConnectionEvent::FullyNegotiatedInbound(inbound) => {
                let setup_msg = inbound.protocol;

                self.pending_events
                    .push(ConnectionHandlerEvent::NotifyBehaviour(
                        SetupHandlerEvent::AppKeyReceived {
                            app_public_key: setup_msg.message.app_public_key.clone(),
                        },
                    ));
            }
            ConnectionEvent::DialUpgradeError(e) => {
                self.pending_events
                    .push(ConnectionHandlerEvent::NotifyBehaviour(
                        SetupHandlerEvent::ErrorDuringSetupHandshake(SetupError::OutboundError(
                            e.error.into(),
                        )),
                    ));
            }
            ConnectionEvent::ListenUpgradeError(e) => {
                self.pending_events
                    .push(ConnectionHandlerEvent::NotifyBehaviour(
                        SetupHandlerEvent::ErrorDuringSetupHandshake(SetupError::InboundError(
                            e.error.into(),
                        )),
                    ));
            }
            _ => {}
        }
    }
}
