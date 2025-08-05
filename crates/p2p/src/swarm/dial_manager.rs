//! Implementation of [`DialManager`].

use std::collections::HashMap;

use cynosure::site_c::queue::Queue;
use libp2p::{identity::PublicKey, swarm::ConnectionId, Multiaddr};

/// [`DialManager`] is responsible for managing the dial process for each peer.
#[derive(Debug, Default)]
pub struct DialManager {
    /// Maps `app_public_key`s to the queue of remaining addresses to try.
    dial_queues: HashMap<PublicKey, Queue<Multiaddr, 4>>,

    /// Maps [`ConnectionId`]s to corresponding `app_public_key`s.
    conn_to_dial: HashMap<ConnectionId, PublicKey>,
}

impl DialManager {
    /// Creates a new empty [`DialManager`].
    pub fn new() -> Self {
        DialManager {
            dial_queues: HashMap::new(),
            conn_to_dial: HashMap::new(),
        }
    }

    /// Inserts a new queue for `app_public_key`.
    pub fn insert_queue(&mut self, app_public_key: PublicKey, queue: Queue<Multiaddr, 4>) {
        self.dial_queues.insert(app_public_key, queue);
    }

    /// Pops the next address from the queue for a given `app_public_key`.
    pub fn pop_next_addr(&mut self, app_public_key: &PublicKey) -> Option<Multiaddr> {
        self.dial_queues
            .get_mut(app_public_key)
            .and_then(|entry| entry.pop_front())
    }

    /// Maps a [`ConnectionId`] to an `app_public_key`.
    pub fn map_connid(&mut self, conn_id: ConnectionId, app_public_key: PublicKey) {
        self.conn_to_dial.insert(conn_id, app_public_key);
    }

    /// Removes while returning the `app_public_key` for a [`ConnectionId`].
    pub fn remove_connid(&mut self, conn_id: &ConnectionId) -> Option<PublicKey> {
        self.conn_to_dial.remove(conn_id)
    }

    /// Removes the queue for an `app_public_key`.
    pub fn remove_queue(&mut self, app_public_key: &PublicKey) {
        self.dial_queues.remove(app_public_key);
    }

    /// Returns the `app_public_key` associated with the given [`ConnectionId`], if any.
    pub fn get_app_public_key_by_connection_id(&self, conn_id: &ConnectionId) -> Option<PublicKey> {
        self.conn_to_dial.get(conn_id).cloned()
    }

    /// Checks if an `app_public_key` already exists in the dial queues.
    pub fn has_app_public_key(&self, app_public_key: &PublicKey) -> bool {
        self.dial_queues.contains_key(app_public_key)
    }
}
