//! Network behavior implementation for the setup protocol.
//!
//! This module provides the [`SetupBehaviour`] which manages the application
//! public key exchange.

#![cfg(feature = "byos")]

use std::{
    collections::HashMap,
    sync::Arc,
    task::{Context, Poll},
};

use libp2p::{
    PeerId,
    core::transport::PortUse,
    identity::PublicKey,
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
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
pub struct SetupBehaviour {
    /// Our Application public key.
    app_public_key: PublicKey,
    /// Our transport id.
    local_transport_id: PeerId,
    /// Object that can sign with Application private key.
    signer: Arc<dyn ApplicationSigner>,
    /// A bimap-like solution for transport_id <-> app_public_keys.
    transport_ids: HashMap<PeerId, PublicKey>,
    app_public_keys: HashMap<PublicKey, PeerId>,
    /// Internal vec of events. Pushed to it in `on_connection_handler_event` and pulled from it
    /// in `poll`.
    events: Vec<SetupBehaviourEvent>,
}

impl SetupBehaviour {
    pub(crate) fn new(
        app_public_key: PublicKey,
        transport_id: PeerId,
        signer: Arc<dyn ApplicationSigner>,
    ) -> Self {
        Self {
            app_public_key,
            local_transport_id: transport_id,
            signer,
            transport_ids: HashMap::new(),
            app_public_keys: HashMap::new(),
            events: Vec::new(),
        }
    }

    /// Gets a corresponding app public key by specific transport id.
    /// Returns None if we don't have the key (not connected or key exchange hasn't happened).
    pub fn get_app_public_key_by_transport_id(&self, peer_id: &PeerId) -> Option<PublicKey> {
        self.transport_ids.get(peer_id).cloned()
    }

    /// Gets a corresponding transport id by specific application public key.
    /// Returns None if we don't have the transport id (not connected or key exchange hasn't
    /// happened).
    pub fn get_transport_id_by_application_key(&self, app_pk: &PublicKey) -> Option<PeerId> {
        self.app_public_keys.get(app_pk).cloned()
    }
}

impl NetworkBehaviour for SetupBehaviour {
    type ConnectionHandler = SetupHandler;
    type ToSwarm = SetupBehaviourEvent;

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        remote_transport_id: PeerId,
        _: &libp2p::Multiaddr,
        _: &libp2p::Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(SetupHandler::new(
            self.app_public_key.clone(),
            self.local_transport_id,
            remote_transport_id,
            self.signer.clone(),
        ))
    }

    fn handle_established_outbound_connection(
        &mut self,
        _: ConnectionId,
        remote_transport_id: PeerId,
        _: &libp2p::Multiaddr,
        _: libp2p::core::Endpoint,
        _: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        Ok(SetupHandler::new(
            self.app_public_key.clone(),
            self.local_transport_id,
            remote_transport_id,
            self.signer.clone(),
        ))
    }

    fn on_swarm_event(&mut self, _: FromSwarm<'_>) {}

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        _: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        match event {
            SetupHandlerEvent::AppKeyReceived { app_public_key } => {
                self.transport_ids.insert(peer_id, app_public_key.clone());
                self.app_public_keys.insert(app_public_key.clone(), peer_id);
                self.events.push(SetupBehaviourEvent::AppKeyReceived {
                    transport_id: peer_id,
                    app_public_key,
                });
            }
            SetupHandlerEvent::ErrorDuringSetupHandshake(error) => {
                self.events
                    .push(SetupBehaviourEvent::ErrorDuringSetupHandshake {
                        transport_id: peer_id,
                        error,
                    });
            }
        }
    }

    fn poll(&mut self, _: &mut Context<'_>) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Some(event) = self.events.pop() {
            return Poll::Ready(ToSwarm::GenerateEvent(event));
        }

        Poll::Pending
    }
}
