//! Network behavior implementation for the setup protocol.
//!
//! This module provides the [`SetupBehaviour`] which manages the setup phase
//! of peer-to-peer connections, including handshake coordination and application
//! public key exchange.

use std::{
    collections::HashMap,
    task::{Context, Poll},
};

use libp2p::{PeerId, core::transport::PortUse, identity::PublicKey, swarm::NetworkBehaviour};

use crate::{
    signer::ApplicationSigner,
    swarm::setup::{events::SetupEvent, handler::SetupHandler},
};

/// Network behavior for managing the setup phase.
#[derive(Debug)]
pub struct SetupBehaviour<S: ApplicationSigner> {
    app_public_key: PublicKey,
    local_transport_id: PeerId,
    signer: S,
    peer_app_keys: HashMap<PeerId, PublicKey>,
    events: Vec<SetupEvent>,
}

impl<S: ApplicationSigner> SetupBehaviour<S> {
    pub(crate) fn new(app_public_key: PublicKey, transport_id: PeerId, signer: S) -> Self {
        Self {
            app_public_key,
            local_transport_id: transport_id,
            signer,
            peer_app_keys: HashMap::new(),
            events: Vec::new(),
        }
    }

    /// Gets the app public key for a specific peer.
    /// Returns None if we don't have the key (not connected or key exchange hasn't happened).
    pub fn get_peer_app_key(&self, peer_id: &PeerId) -> Option<PublicKey> {
        self.peer_app_keys.get(peer_id).cloned()
    }

    /// Sets the local peer ID (for compatibility with existing code).
    pub fn set_local_peer_id(&mut self, peer_id: PeerId) {
        self.local_transport_id = peer_id;
    }
}

impl<S: ApplicationSigner> NetworkBehaviour for SetupBehaviour<S> {
    type ConnectionHandler = SetupHandler<S>;
    type ToSwarm = SetupEvent;

    fn handle_established_inbound_connection(
        &mut self,
        _: libp2p::swarm::ConnectionId,
        remote_transport_id: PeerId,
        _: &libp2p::Multiaddr,
        _: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(
            SetupHandler::new(self.app_public_key.clone(), self.local_transport_id, remote_transport_id, self.signer.clone())
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: libp2p::swarm::ConnectionId,
        remote_transport_id: PeerId,
        _: &libp2p::Multiaddr,
        _: libp2p::core::Endpoint,
        _: PortUse,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(
            SetupHandler::new(self.app_public_key.clone(), self.local_transport_id, remote_transport_id, self.signer.clone())
        )
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm<'_>) {
        match event {
            libp2p::swarm::FromSwarm::ConnectionEstablished(_event) => {}
            _ => {}
        }
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _: libp2p::swarm::ConnectionId,
        event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        match event {
            SetupEvent::AppKeyReceived { app_public_key, .. } => {
                self.peer_app_keys.insert(peer_id, app_public_key.clone());
                self.events.push(SetupEvent::AppKeyReceived {
                    peer_id,
                    app_public_key,
                });
            }
            SetupEvent::HandshakeComplete { .. } => {
                self.events.push(SetupEvent::HandshakeComplete { peer_id });
            }
            SetupEvent::SignatureVerificationFailed { error, .. } => {
                self.events
                    .push(SetupEvent::SignatureVerificationFailed { peer_id, error });
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
