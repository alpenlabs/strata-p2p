//! Event types for the setup protocol.
//!
//! This module defines the events that are emitted during the setup phase
//! of peer-to-peer connections, providing information about handshake progress
//! and key exchange completion.

#![cfg(feature = "byos")]

use libp2p::{PeerId, identity::PublicKey, swarm::ConnectionId};

use crate::swarm::errors::SetupError;

/// Events emitted during the setup phase of peer connections.
///
/// These events are essentially wrappers around [`SetupHandlerEvent`] that just add transport_id
/// aka peer_id. These events handled in behaviour's `on_connection_handler_event`, then are taken
/// by swarm in `poll()` so they are going to next event from swarm, so that logic around knowing
/// the SetupBehaviour (get remote's peer application public key) can be implemented in
/// `crates/p2p/src/swarm/mod.rs`.
#[derive(Debug)]
pub enum SetupBehaviourEvent {
    /// Indicates that an application public key has been received from a peer.
    ///
    /// This event is fired when the local node successfully receives and
    /// processes a peer's application public key during the handshake.
    AppKeyReceived {
        transport_id: PeerId,
        app_public_key: PublicKey,
        conn_id: ConnectionId,
    },

    /// Emitted when the setup protocol negotiation fails with a remote peer.
    ///
    /// This event indicates that the peers could not agree on a common
    /// version of the setup protocol or that we support Setup, but remote peer does not support
    /// Setup.
    NegotiationFailed {
        transport_id: PeerId,
        conn_id: ConnectionId,
    },

    /// Indicates that signature verification failed.
    ///
    /// This event is fired when the signature verification fails for a peer's
    /// setup message, indicating the connection should be dropped.
    ErrorDuringSetupHandshake {
        transport_id: PeerId,
        conn_id: ConnectionId,
        error: SetupError,
    },
}

/// Events emitted during the setup phase of peer connections.
///
/// These events are generated at connection handler in `on_connection_event` and are taken by
/// swarm in connection handler poll() . Then these appears in behaviour in
/// `on_connection_handler_event`.
#[derive(Debug)]
pub enum SetupHandlerEvent {
    /// Indicates that an application public key has been received from a peer.
    ///
    /// This event is fired when the local node successfully receives and
    /// processes a peer's application public key during the handshake.
    AppKeyReceived { app_public_key: PublicKey },

    /// Emitted when the setup protocol negotiation fails with a remote peer.
    ///
    /// This event indicates that the peers could not agree on a common
    /// version of the setup protocol or that we support Setup, but remote peer does not support
    /// Setup.
    ///
    /// At the moment of writing(end of Aug 2025) we just disconnect from the peer.
    NegotiationFailed,

    /// Something has failed during setup handshake.
    ErrorDuringSetupHandshake(SetupError),
}
