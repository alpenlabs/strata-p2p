//! Connection handler for the setup protocol.
//!
//! This module provides the [`SetupHandler`] which manages individual connection
//! setup processes, handling both inbound and outbound substreams for the handshake
//! protocol.

use std::task::{Context, Poll};

use libp2p::{
    PeerId,
    identity::PublicKey,
    swarm::{ConnectionHandler, ConnectionHandlerEvent, SubstreamProtocol},
};

use crate::{
    signer::ApplicationSigner,
    swarm::setup::{
        events::SetupHandlerEvent,
        upgrade::{InboundSetupUpgrade, OutboundSetupUpgrade},
    },
};

/// Connection handler for managing setup protocol substreams.
///
/// This handler manages the lifecycle of setup substreams for a single connection,
/// coordinating both inbound and outbound handshake processes.
#[derive(Debug)]
pub struct SetupHandler<S: ApplicationSigner> {
    outbound_substream: Option<SubstreamProtocol<OutboundSetupUpgrade<S>, ()>>,
    pending_events: Vec<ConnectionHandlerEvent<OutboundSetupUpgrade<S>, (), SetupHandlerEvent>>,
}

impl<S: ApplicationSigner> SetupHandler<S> {
    pub(super) fn new(
        app_public_key: PublicKey,
        local_transport_id: PeerId,
        remote_transport_id: PeerId,
        signer: S,
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

impl<S: ApplicationSigner> ConnectionHandler for SetupHandler<S> {
    type FromBehaviour = ();
    type ToBehaviour = SetupHandlerEvent;
    type InboundProtocol = InboundSetupUpgrade;
    type OutboundProtocol = OutboundSetupUpgrade<S>;
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
        event: libp2p::swarm::handler::ConnectionEvent<
            '_,
            Self::InboundProtocol,
            Self::OutboundProtocol,
        >,
    ) {
        match event {
            libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedInbound(inbound) => {
                let (setup_msg, signature_valid) = inbound.protocol;

                if !signature_valid {
                    self.pending_events
                        .push(ConnectionHandlerEvent::NotifyBehaviour(
                            SetupHandlerEvent::SignatureVerificationFailed {
                                error: "Signature verification failed".to_string(),
                            },
                        ));
                    return;
                }

                // Validate message format
                if let Err(e) = setup_msg.validate() {
                    self.pending_events
                        .push(ConnectionHandlerEvent::NotifyBehaviour(
                            SetupHandlerEvent::SignatureVerificationFailed {
                                error: format!("Message validation failed: {e}"),
                            },
                        ));
                    return;
                }

                // Get public key
                let app_public_key = match setup_msg.get_app_public_key() {
                    Ok(key) => key,
                    Err(e) => {
                        self.pending_events
                            .push(ConnectionHandlerEvent::NotifyBehaviour(
                                SetupHandlerEvent::SignatureVerificationFailed {
                                    error: format!("Invalid public key: {e}"),
                                },
                            ));
                        return;
                    }
                };

                // Signature is already verified, so we can accept the message
                self.pending_events
                    .push(ConnectionHandlerEvent::NotifyBehaviour(
                        SetupHandlerEvent::AppKeyReceived { app_public_key },
                    ));
            }
            libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedOutbound(_outbound) => {
                self.pending_events
                    .push(ConnectionHandlerEvent::NotifyBehaviour(
                        SetupHandlerEvent::HandshakeComplete {},
                    ));
            }
            libp2p::swarm::handler::ConnectionEvent::DialUpgradeError(error) => {
                // Report as signature verification failure for signing errors
                self.pending_events
                    .push(ConnectionHandlerEvent::NotifyBehaviour(
                        SetupHandlerEvent::SignatureVerificationFailed {
                            error: format!("Outbound signing failed: {}", error.error),
                        },
                    ));
            }
            libp2p::swarm::handler::ConnectionEvent::ListenUpgradeError(error) => {
                // Report as signature verification failure for verification errors
                self.pending_events
                    .push(ConnectionHandlerEvent::NotifyBehaviour(
                        SetupHandlerEvent::SignatureVerificationFailed {
                            error: format!("Inbound verification failed: {}", error.error),
                        },
                    ));
            }
            _ => {}
        }
    }
}
