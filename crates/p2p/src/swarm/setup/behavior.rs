//! Network behavior implementation for the setup protocol.
//!
//! This module provides the [`SetupBehaviour`] which manages the application
//! public key exchange.

use std::{
    collections::HashMap,
    task::{Context, Poll},
};

use libp2p::{PeerId, core::transport::PortUse, identity::PublicKey, swarm::NetworkBehaviour};

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

    // Note(Arniiiii): Ill'a that he doesn't like bimap crate, and I had to do this...
    tid_to_app_pk: HashMap<PeerId, PublicKey>,
    app_pk_to_tid: HashMap<PublicKey, PeerId>,

    events: Vec<SetupBehaviourEvent>,
}

impl<S: ApplicationSigner> SetupBehaviour<S> {
    pub(crate) fn new(app_public_key: PublicKey, transport_id: PeerId, signer: S) -> Self {
        Self {
            app_public_key,
            local_transport_id: transport_id,
            signer,
            tid_to_app_pk: HashMap::new(),
            app_pk_to_tid: HashMap::new(),
            events: Vec::new(),
        }
    }

    /// Gets a corresponding app public key by specific transport id.
    /// Returns None if we don't have the key (not connected or key exchange hasn't happened).
    pub fn get_app_public_key_by_transport_id(&self, peer_id: &PeerId) -> Option<PublicKey> {
        self.tid_to_app_pk.get(peer_id).cloned()
    }

    /// Gets a corresponding transport id by specific application public key.
    /// Returns None if we don't have the transport id (not connected or key exchange hasn't
    /// happened).
    pub fn get_transport_id_by_app_pk(&self, app_pk: &PublicKey) -> Option<PeerId> {
        self.app_pk_to_tid.get(app_pk).cloned()
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

    fn on_swarm_event(&mut self, _: libp2p::swarm::FromSwarm<'_>) {}

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _: libp2p::swarm::ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        match event {
            SetupHandlerEvent::AppKeyReceived { app_public_key } => {
                self.tid_to_app_pk.insert(peer_id, app_public_key.clone());
                self.app_pk_to_tid.insert(app_public_key.clone(), peer_id);
                self.events.push(SetupBehaviourEvent::AppKeyReceived {
                    transport_id: peer_id,
                    app_public_key,
                });
            }
            SetupHandlerEvent::HandshakeComplete => {
                self.events.push(SetupBehaviourEvent::HandshakeComplete {
                    transport_id: peer_id,
                });
            }
            SetupHandlerEvent::ErrorDuringHandshake(error) => {
                self.events.push(SetupBehaviourEvent::ErrorDuringHandshake {
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
        if let Some(event) = self.events.pop() {
            return Poll::Ready(libp2p::swarm::ToSwarm::GenerateEvent(event));
        }

        Poll::Pending
    }
}
