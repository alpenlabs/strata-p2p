//! Connection handler for the setup protocol.
//!
//! This module provides the [`SetupHandler`] which manages individual connection
//! setup processes, handling both inbound and outbound substreams for the handshake
//! protocol.

use std::task::{Context, Poll};

use libp2p::{
    PeerId,
    identity::ed25519::PublicKey,
    swarm::{ConnectionHandler, ConnectionHandlerEvent, SubstreamProtocol},
};

use crate::swarm::setup::{
    events::SetupEvent,
    upgrade::{InboundSetupUpgrade, OutboundSetupUpgrade},
};

/// Connection handler for managing setup protocol substreams.
///
/// This handler manages the lifecycle of setup substreams for a single connection,
/// coordinating both inbound and outbound handshake processes.
#[derive(Debug)]
pub struct SetupHandler {
    outbound_substream: Option<SubstreamProtocol<OutboundSetupUpgrade, ()>>,
    pending_events: Vec<ConnectionHandlerEvent<OutboundSetupUpgrade, (), SetupEvent>>,
}

impl SetupHandler {
    pub(super) fn new(app_public_key: PublicKey) -> Self {
        Self {
            outbound_substream: Some(SubstreamProtocol::new(
                OutboundSetupUpgrade::new(app_public_key),
                (),
            )),
            pending_events: Vec::new(),
        }
    }
}

impl ConnectionHandler for SetupHandler {
    type FromBehaviour = ();
    type ToBehaviour = SetupEvent;
    type InboundProtocol = InboundSetupUpgrade;
    type OutboundProtocol = OutboundSetupUpgrade;
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
                let handshake_msg = inbound.protocol;
                if let Ok(public_key) = PublicKey::try_from_bytes(&handshake_msg.app_public_key) {
                    self.pending_events
                        .push(ConnectionHandlerEvent::NotifyBehaviour(
                            SetupEvent::AppKeyReceived {
                                peer_id: PeerId::random(),
                                app_public_key: public_key,
                            },
                        ));
                }
            }
            libp2p::swarm::handler::ConnectionEvent::FullyNegotiatedOutbound(_outbound) => {
                self.pending_events
                    .push(ConnectionHandlerEvent::NotifyBehaviour(
                        SetupEvent::HandshakeComplete {
                            peer_id: PeerId::random(),
                        },
                    ));
            }
            libp2p::swarm::handler::ConnectionEvent::DialUpgradeError(_) => {
                // Handle dial upgrade error
            }
            libp2p::swarm::handler::ConnectionEvent::ListenUpgradeError(_) => {
                // Handle listen upgrade error
            }
            _ => {}
        }
    }
}
