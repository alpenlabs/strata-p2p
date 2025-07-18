//! Network behavior implementation for the setup protocol.
//!
//! This module provides the [`SetupBehaviour`] which manages the setup phase
//! of peer-to-peer connections, including handshake coordination and application
//! public key exchange.

use std::{
    collections::HashSet,
    task::{Context, Poll},
};

use bimap::BiHashMap;
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

/// Filtering: an interface for allowlist,banlist and maybe in future scoring
pub trait Filtering: std::fmt::Debug + Send + Sync + Clone + 'static {
    /// Adds an application's public key to the list of respected keys,
    /// allowing connections from this application.
    fn respect_app_pk_to_allow_connection(&mut self, app_pk: PublicKey);

    /// Removes an application's public key from the list of respected keys,
    /// disallowing connections from this application.
    fn disrespect_app_pk_to_disallow_connection(&mut self, app_pk: PublicKey);

    /// Checks if a given application's public key is currently respected,
    /// indicating whether a connection from it is allowed.
    ///
    /// Returns `true` if the application's public key is respected, `false` otherwise.
    fn is_app_pk_respected_enough_to_connect(&self, app_pk: &PublicKey) -> bool;
}

/// Allow list aka Whitelist.
#[derive(Debug, Clone)]
pub struct AllowList {
    /// Set of app_pk in allow list.
    app_pk_allowlist: HashSet<PublicKey>,
}

impl AllowList {
    /// Create filtering "Allowlist" with predefined allow list.
    pub const fn new(pre_defined_allow_list: HashSet<PublicKey>) -> Self {
        AllowList {
            app_pk_allowlist: pre_defined_allow_list,
        }
    }
}

impl Filtering for AllowList {
    fn respect_app_pk_to_allow_connection(&mut self, app_pk: PublicKey) {
        self.app_pk_allowlist.insert(app_pk);
    }

    fn disrespect_app_pk_to_disallow_connection(&mut self, app_pk: PublicKey) {
        self.app_pk_allowlist.remove(&app_pk);
    }

    fn is_app_pk_respected_enough_to_connect(&self, app_pk: &PublicKey) -> bool {
        self.app_pk_allowlist.contains(app_pk)
    }
}

/// Ban list aka blacklist.
#[derive(Debug, Clone)]
pub struct BanList {
    /// Set of app_pk in ban list.
    app_pk_banlist: HashSet<PublicKey>,
}

impl BanList {
    /// Create filtering "Banlist" with predefined ban list.
    pub const fn new(pre_defined_ban_list: HashSet<PublicKey>) -> Self {
        BanList {
            app_pk_banlist: pre_defined_ban_list,
        }
    }
}

impl Filtering for BanList {
    fn respect_app_pk_to_allow_connection(&mut self, app_pk: PublicKey) {
        // To respect an app_pk in a banlist, remove it from the banlist.
        self.app_pk_banlist.remove(&app_pk);
    }

    fn disrespect_app_pk_to_disallow_connection(&mut self, app_pk: PublicKey) {
        // To disrespect an app_pk in a banlist, add it to the banlist.
        self.app_pk_banlist.insert(app_pk);
    }

    fn is_app_pk_respected_enough_to_connect(&self, app_pk: &PublicKey) -> bool {
        // An app_pk is respected enough to connect if it's NOT in the banlist.
        !self.app_pk_banlist.contains(app_pk)
    }
}

/// Network behavior for managing the setup phase.
#[derive(Debug)]
pub struct SetupBehaviour<S: ApplicationSigner, F: Filtering> {
    app_public_key: PublicKey,
    local_transport_id: PeerId,
    signer: S,
    peer_app_keys: BiHashMap<PeerId, PublicKey>,
    events: Vec<SetupBehaviourEvent>,
    events_toswarm_unwrapped: Vec<ToSwarm<SetupBehaviourEvent, ()>>,
    app_pk_filtering: F,
}

impl<S: ApplicationSigner, F: Filtering> SetupBehaviour<S, F> {
    pub(crate) fn new(
        app_public_key: PublicKey,
        transport_id: PeerId,
        signer: S,
        app_pk_filtering: F,
    ) -> Self {
        Self {
            app_public_key,
            local_transport_id: transport_id,
            signer,
            peer_app_keys: BiHashMap::new(),
            events: Vec::new(),
            events_toswarm_unwrapped: Vec::new(),
            app_pk_filtering,
        }
    }

    /// Gets the peerid by a specific app public key.
    /// Returns None if we don't have the key (not connected or key exchange hasn't happened).
    pub fn get_app_key_by_peer_id(&self, peer_id: &PeerId) -> Option<PublicKey> {
        self.peer_app_keys.get_by_left(peer_id).cloned()
    }

    /// Gets the app public key by a specific peerid.
    /// Returns None if we don't have the key (not connected or key exchange hasn't happened).
    pub fn get_peer_id_by_app_pk(&self, app_public_key: &PublicKey) -> Option<PeerId> {
        self.peer_app_keys.get_by_right(app_public_key).cloned()
    }

    /// Sets the local peer ID (for compatibility with existing code).
    #[expect(clippy::missing_const_for_fn, reason = "false positive")]
    pub fn set_local_peer_id(&mut self, peer_id: PeerId) {
        self.local_transport_id = peer_id;
    }

    /// Copy filtering.
    pub fn get_whole_filtering(self) -> F {
        self.app_pk_filtering
    }

    /// Const access to filtering.
    pub const fn get_access_whole_filtering(&self) -> &F {
        &self.app_pk_filtering
    }

    /// Mutable access to filtering.
    #[expect(clippy::missing_const_for_fn, reason = "false positive")]
    pub fn get_mut_access_whole_filtering(&mut self) -> &mut F {
        &mut self.app_pk_filtering
    }
}

impl<S: ApplicationSigner, F: Filtering> NetworkBehaviour for SetupBehaviour<S, F> {
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
                match self
                    .app_pk_filtering
                    .is_app_pk_respected_enough_to_connect(&app_public_key)
                {
                    true => {
                        self.events.push(SetupBehaviourEvent::AppKeyReceived {
                            peer_id,
                            app_public_key,
                        });
                    }
                    false => {
                        self.events.push(
                            SetupBehaviourEvent::AttemptConnectToPeerNotInAllowedList {
                                peer_id,
                                app_public_key,
                            },
                        );
                        self.events_toswarm_unwrapped
                            .push(ToSwarm::CloseConnection {
                                peer_id,
                                connection: libp2p::swarm::CloseConnection::All,
                            });
                    }
                };
            }
            SetupHandlerEvent::HandshakeComplete => {
                self.events
                    .push(SetupBehaviourEvent::HandshakeComplete { peer_id });
            }
            SetupHandlerEvent::SignatureVerificationFailed { error, .. } => {
                self.events
                    .push(SetupBehaviourEvent::SignatureVerificationFailed { peer_id, error });
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
