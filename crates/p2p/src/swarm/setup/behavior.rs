//! Network behavior implementation for the setup protocol.
//!
//! This module provides the [`SetupBehaviour`] which manages the setup phase
//! of peer-to-peer connections, including handshake coordination and application
//! public key exchange.

use std::{
    collections::{HashMap, HashSet},
    task::{Context, Poll},
};

use libp2p::{
    PeerId,
    core::transport::PortUse,
    identity::PublicKey,
    swarm::{NetworkBehaviour, ToSwarm},
};

use crate::{
    signer::ApplicationSigner,
    swarm::setup::{
        events::{SetupBehaviourEvent, SetupHandlerEvent},
        handler::SetupHandler,
    },
};

/// Network behavior for managing the setup phase.
#[derive(Debug)]
pub struct SetupBehaviour<S: ApplicationSigner> {
    app_public_key: PublicKey,
    local_transport_id: PeerId,
    signer: S,
    peer_app_keys: HashMap<PeerId, PublicKey>,
    events: Vec<SetupBehaviourEvent>,
    events_toswarm_unwrapped: Vec<ToSwarm<SetupBehaviourEvent, ()>>,
    app_pk_allow_list: HashSet<PublicKey>,
}

impl<S: ApplicationSigner> SetupBehaviour<S> {
    pub(crate) fn new(
        app_public_key: PublicKey,
        transport_id: PeerId,
        signer: S,
        app_pk_allow_list: HashSet<PublicKey>,
    ) -> Self {
        Self {
            app_public_key,
            local_transport_id: transport_id,
            signer,
            peer_app_keys: HashMap::new(),
            events: Vec::new(),
            events_toswarm_unwrapped: Vec::new(),
            app_pk_allow_list,
        }
    }

    /// Gets the peerid by a specific app public key.
    /// Returns None if we don't have the key (not connected or key exchange hasn't happened).
    pub fn get_app_public_key_by_transport_id(&self, peer_id: &PeerId) -> Option<PublicKey> {
        self.peer_app_keys.get(peer_id).cloned()
    }
}

impl<S: ApplicationSigner> NetworkBehaviour for SetupBehaviour<S> {
    type ConnectionHandler = SetupHandler<S>;
    type ToSwarm = SetupBehaviourEvent;

    fn handle_established_inbound_connection(
        &mut self,
        _: libp2p::swarm::ConnectionId,
        remote_transport_id: PeerId,
        _: &libp2p::Multiaddr,
        _: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(SetupHandler::new(
            self.app_public_key.clone(),
            self.local_transport_id,
            remote_transport_id,
            self.signer.clone(),
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: libp2p::swarm::ConnectionId,
        remote_transport_id: PeerId,
        _: &libp2p::Multiaddr,
        _: libp2p::core::Endpoint,
        _: PortUse,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(SetupHandler::new(
            self.app_public_key.clone(),
            self.local_transport_id,
            remote_transport_id,
            self.signer.clone(),
        ))
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm<'_>) {
        if let libp2p::swarm::FromSwarm::ConnectionEstablished(_event) = event {}
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _: libp2p::swarm::ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        match event {
            SetupHandlerEvent::AppKeyReceived { app_public_key } => {
                self.peer_app_keys.insert(peer_id, app_public_key.clone());
                match self.app_pk_allow_list.contains(&app_public_key) {
                    true => {
                        self.events.push(SetupBehaviourEvent::AppKeyReceived {
                            transport_id: peer_id,
                            app_public_key,
                        });
                    }
                    false => {
                        self.events
                            .push(SetupBehaviourEvent::AttemptConnectToDisrespectedPeer {
                                transport_id: peer_id,
                                app_public_key,
                            });
                        self.events_toswarm_unwrapped
                            .push(ToSwarm::CloseConnection {
                                peer_id,
                                connection: libp2p::swarm::CloseConnection::All,
                            });
                    }
                };
            }
            SetupHandlerEvent::HandshakeComplete => {
                self.events.push(SetupBehaviourEvent::HandshakeComplete {
                    transport_id: peer_id,
                });
            }
            SetupHandlerEvent::SignatureVerificationFailed { error, .. } => {
                self.events
                    .push(SetupBehaviourEvent::SignatureVerificationFailed {
                        transport_id: peer_id,
                        error,
                    });
            }
        }
    }

    fn poll(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<libp2p::swarm::ToSwarm<Self::ToSwarm, libp2p::swarm::THandlerInEvent<Self>>> {
        if let Some(event) = self.events_toswarm_unwrapped.pop() {
            return Poll::Ready(event);
        }

        if let Some(event) = self.events.pop() {
            return Poll::Ready(libp2p::swarm::ToSwarm::GenerateEvent(event));
        }
        Poll::Pending
    }
}
