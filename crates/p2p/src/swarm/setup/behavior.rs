//! Network behavior implementation for the setup protocol.
//!
//! This module provides the [`SetupBehaviour`] which manages the setup phase
//! of peer-to-peer connections, including handshake coordination and application
//! public key exchange.

use std::{
    collections::HashMap,
    task::{Context, Poll},
};

use libp2p::{
    PeerId, core::transport::PortUse, identity::PublicKey, swarm::NetworkBehaviour,
};

use crate::swarm::setup::{events::SetupEvent, handler::SetupHandler};

/// Network behavior for managing the setup phase.
#[derive(Debug)]
pub struct SetupBehaviour {
    app_public_key: PublicKey,
    peer_app_keys: HashMap<PeerId, PublicKey>,
    events: Vec<SetupEvent>,
}

impl SetupBehaviour {
    pub(crate) fn new(app_public_key: PublicKey) -> Self {
        Self {
            app_public_key,
            peer_app_keys: HashMap::new(),
            events: Vec::new(),
        }
    }

}

impl NetworkBehaviour for SetupBehaviour {
    type ConnectionHandler = SetupHandler;
    type ToSwarm = SetupEvent;

    fn handle_established_inbound_connection(
        &mut self,
        _: libp2p::swarm::ConnectionId,
        _: PeerId,
        _: &libp2p::Multiaddr,
        _: &libp2p::Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(SetupHandler::new(self.app_public_key.clone()))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: libp2p::swarm::ConnectionId,
        _: PeerId,
        _: &libp2p::Multiaddr,
        _: libp2p::core::Endpoint,
        _: PortUse,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        Ok(SetupHandler::new(self.app_public_key.clone()))
    }

    fn on_swarm_event(&mut self, _: libp2p::swarm::FromSwarm<'_>) {}

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
