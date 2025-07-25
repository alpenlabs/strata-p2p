//! Implementation of DialManager

use std::{collections::VecDeque, sync::Arc};

use crossbeam_deque::Injector;
use dashmap::DashMap;
use libp2p::{Multiaddr, identity::PublicKey, swarm::ConnectionId};

/// Manages the state for multi-address dial attempts, including address queues and connection ID
/// mappings. Used to coordinate retries and track which connection corresponds to which dial
/// sequence.
///
/// This implementation uses lock-free concurrent data structures to eliminate deadlock risks:
/// - `DashMap` for connection ID mappings (lock-free concurrent HashMap)
/// - `crossbeam-deque` Injector for thread-safe concurrent queues
#[derive(Debug)]
pub struct DialManager {
    /// Maps an app_public_key to the queue of remaining addresses to try.
    dial_queues: Arc<DashMap<PublicKey, Injector<Multiaddr>>>,
    /// Maps a libp2p connection ID to the corresponding app_public_key.
    conn_to_dial: Arc<DashMap<ConnectionId, PublicKey>>,
}

impl Default for DialManager {
    fn default() -> Self {
        Self::new()
    }
}

impl DialManager {
    /// Creates a new, empty DialManager.
    pub fn new() -> Self {
        DialManager {
            dial_queues: Arc::new(DashMap::new()),
            conn_to_dial: Arc::new(DashMap::new()),
        }
    }

    /// Insert a new queue for a dial sequence id
    pub fn insert_queue(&self, app_public_key: PublicKey, addresses: VecDeque<Multiaddr>) {
        let queue = Injector::new();

        // Push all addresses to the deque
        for addr in addresses {
            queue.push(addr);
        }

        self.dial_queues.insert(app_public_key, queue);
    }

    /// Pop the next address from the queue for a given app_public_key
    pub fn pop_next_addr(&self, app_public_key: &PublicKey) -> Option<Multiaddr> {
        self.dial_queues
            .get(app_public_key)
            .and_then(|entry| entry.value().steal().success())
    }

    /// Map a connection id to an app_public_key
    pub fn map_connid(&self, conn_id: ConnectionId, app_public_key: PublicKey) {
        self.conn_to_dial.insert(conn_id, app_public_key);
    }

    /// Remove and get the app_public_key for a connection id
    pub fn remove_connid(&self, conn_id: &ConnectionId) -> Option<PublicKey> {
        self.conn_to_dial.remove(conn_id).map(|(_, value)| value)
    }

    /// Remove the queue for an app_public_key
    pub fn remove_queue(&self, app_public_key: &PublicKey) {
        self.dial_queues.remove(app_public_key);
    }

    /// Returns the app_public_key associated with the given connection ID, if any.
    pub fn get_app_public_key_by_connection_id(&self, conn_id: &ConnectionId) -> Option<PublicKey> {
        self.conn_to_dial
            .get(conn_id)
            .map(|entry| entry.value().clone())
    }

    /// Check if an app_public_key already exists in the dial queues
    pub fn has_app_public_key(&self, app_public_key: &PublicKey) -> bool {
        self.dial_queues.contains_key(app_public_key)
    }
}
