//! Implementation of DialManager

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use libp2p::{Multiaddr, identity::PublicKey, swarm::ConnectionId};
use tokio::sync::Mutex;

/// Manages the state for multi-address dial attempts, including address queues and connection ID
/// mappings. Used to coordinate retries and track which connection corresponds to which dial
/// sequence.
#[derive(Debug)]
pub struct DialManager {
    /// Maps an app_pk (PublicKey) to the queue of remaining addresses to try.
    pub dial_queues: Arc<Mutex<HashMap<PublicKey, VecDeque<Multiaddr>>>>,
    /// Maps a libp2p connection ID to the corresponding app_pk (PublicKey).
    pub conn_to_dial: Arc<Mutex<HashMap<ConnectionId, PublicKey>>>,
}

impl DialManager {
    /// Creates a new, empty DialManager.
    pub fn new() -> Self {
        DialManager {
            dial_queues: Arc::new(Mutex::new(HashMap::new())),
            conn_to_dial: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Insert a new queue for a dial sequence id
    pub async fn insert_queue(&self, app_pk: PublicKey, addresses: Vec<Multiaddr>) {
        let mut queues = self.dial_queues.lock().await;
        queues.insert(app_pk, VecDeque::from(addresses));
    }

    /// Pop the next address from the queue for a given app_pk (PublicKey)
    pub async fn pop_next_addr(&self, app_pk: &PublicKey) -> Option<Multiaddr> {
        let mut queues = self.dial_queues.lock().await;
        if let Some(queue) = queues.get_mut(app_pk) {
            queue.pop_front()
        } else {
            None
        }
    }

    /// Map a connection id to an app_pk (PublicKey)
    pub async fn map_connid(&self, conn_id: ConnectionId, app_pk: PublicKey) {
        let mut map = self.conn_to_dial.lock().await;
        map.insert(conn_id, app_pk);
    }

    /// Remove and get the app_pk (PublicKey) for a connection id
    pub async fn remove_connid(&self, conn_id: &ConnectionId) -> Option<PublicKey> {
        let mut map = self.conn_to_dial.lock().await;
        map.remove(conn_id)
    }

    /// Remove the queue for an app_pk (PublicKey)
    pub async fn remove_queue(&self, app_pk: &PublicKey) {
        let mut queues = self.dial_queues.lock().await;
        queues.remove(app_pk);
    }

    /// Returns the app_pk (PublicKey) associated with the given connection ID, if any.
    pub async fn get_app_pk_by_connection_id(&self, conn_id: &ConnectionId) -> Option<PublicKey> {
        let map = self.conn_to_dial.lock().await;
        map.get(conn_id).cloned()
    }
}

impl Default for DialManager {
    fn default() -> Self {
        Self::new()
    }
}
