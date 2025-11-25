//! Connection handler for the setup protocol.
//!
//! This module provides the [`SetupHandler`] which manages individual connection
//! setup processes, handling both inbound and outbound substreams for the setup
//! protocol.

#![cfg(feature = "byos")]

use std::{
    sync::Arc,
    task::{Context, Poll},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use libp2p::{
    PeerId,
    identity::PublicKey,
    swarm::{
        ConnectionHandler, ConnectionHandlerEvent, SubstreamProtocol, handler::ConnectionEvent,
    },
};
use tracing::warn;

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
#[expect(clippy::type_complexity)]
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

    /// Our local transport ID (for validation).
    local_transport_id: PeerId,

    /// Remote peer's transport ID (for validation).
    remote_transport_id: PeerId,

    /// Maximum age for setup messages.
    envelope_max_age: Duration,

    /// Maximum allowed clock skew for future timestamps.
    max_clock_skew: Duration,

    /// Whether to keep the connection alive.
    keep_alive: bool,
}

impl SetupHandler {
    pub(crate) fn new(
        app_public_key: PublicKey,
        local_transport_id: PeerId,
        remote_transport_id: PeerId,
        signer: Arc<dyn ApplicationSigner>,
        envelope_max_age: Duration,
        max_clock_skew: Duration,
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
            local_transport_id,
            remote_transport_id,
            envelope_max_age,
            max_clock_skew,
            keep_alive: true,
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
        self.keep_alive
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
                self.keep_alive = false;
                let signed_setup_msg = inbound.protocol;

                // Verify signature
                let signature_valid = signed_setup_msg.verify().unwrap_or(false);

                if !signature_valid {
                    warn!(
                        remote_transport_id = %self.remote_transport_id,
                        "Setup message signature verification failed"
                    );
                    self.pending_events
                        .push(ConnectionHandlerEvent::NotifyBehaviour(
                            SetupHandlerEvent::ErrorDuringSetupHandshake(
                                SetupError::SignatureVerificationFailed,
                            ),
                        ));
                    return;
                }

                let setup_message = signed_setup_msg.message;

                // Validate transport IDs match the actual connection
                // The remote's local_transport_id should match our view of their
                // remote_transport_id The remote's remote_transport_id should match
                // our local_transport_id
                if setup_message.remote_transport_id != self.local_transport_id
                    || setup_message.local_transport_id != self.remote_transport_id
                {
                    warn!(
                        remote_transport_id = %self.remote_transport_id,
                        msg_local_transport_id = %setup_message.local_transport_id,
                        msg_remote_transport_id = %setup_message.remote_transport_id,
                        our_local_transport_id = %self.local_transport_id,
                        "Transport ID mismatch in setup message"
                    );
                    self.pending_events
                        .push(ConnectionHandlerEvent::NotifyBehaviour(
                            SetupHandlerEvent::ErrorDuringSetupHandshake(
                                SetupError::TransportIdMismatch,
                            ),
                        ));
                    return;
                }

                // Validate message timestamp
                let now_secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();

                // Check if message is too old
                let age = Duration::from_secs(now_secs.saturating_sub(setup_message.date));
                if age > self.envelope_max_age {
                    warn!(
                        remote_transport_id = %self.remote_transport_id,
                        ?age,
                        max_age = ?self.envelope_max_age,
                        "Setup message is too old"
                    );
                    self.pending_events
                        .push(ConnectionHandlerEvent::NotifyBehaviour(
                            SetupHandlerEvent::ErrorDuringSetupHandshake(SetupError::StaleMessage),
                        ));
                    return;
                }

                // Check if message is too far in the future
                if setup_message.date > now_secs {
                    let future_delta = Duration::from_secs(setup_message.date - now_secs);
                    if future_delta > self.max_clock_skew {
                        warn!(
                            remote_transport_id = %self.remote_transport_id,
                            future_delta = ?future_delta,
                            max_clock_skew = ?self.max_clock_skew,
                            "Setup message timestamp too far in future"
                        );
                        self.pending_events
                            .push(ConnectionHandlerEvent::NotifyBehaviour(
                                SetupHandlerEvent::ErrorDuringSetupHandshake(
                                    SetupError::FutureMessage,
                                ),
                            ));
                        return;
                    }
                }

                // All validation passed, accept the app key
                self.pending_events
                    .push(ConnectionHandlerEvent::NotifyBehaviour(
                        SetupHandlerEvent::AppKeyReceived {
                            app_public_key: setup_message.app_public_key.clone(),
                        },
                    ));
            }
            ConnectionEvent::DialUpgradeError(e) => {
                self.keep_alive = false;
                let event = match e.error {
                    libp2p::swarm::StreamUpgradeError::NegotiationFailed => {
                        SetupHandlerEvent::NegotiationFailed
                    }
                    _ => SetupHandlerEvent::ErrorDuringSetupHandshake(SetupError::OutboundError(
                        e.error.into(),
                    )),
                };
                self.pending_events
                    .push(ConnectionHandlerEvent::NotifyBehaviour(event));
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
