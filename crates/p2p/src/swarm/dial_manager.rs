//! Implementation of DialManager

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use libp2p::{Multiaddr, swarm::ConnectionId};
use tokio::sync::Mutex;

/// Manages the state for multi-address dial attempts, including address queues and connection ID
/// mappings. Used to coordinate retries and track which connection corresponds to which dial
/// sequence.
#[derive(Debug)]
pub struct DialManager {
    /// Maps a unique dial sequence ID to the queue of remaining addresses to try.
    pub dial_queues: Arc<Mutex<HashMap<u32, VecDeque<Multiaddr>>>>,
    /// Maps a libp2p connection ID to the corresponding dial sequence ID.
    pub conn_to_dial: Arc<Mutex<HashMap<ConnectionId, u32>>>,
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
    pub async fn insert_queue(&self, id: u32, addresses: Vec<Multiaddr>) {
        let mut queues = self.dial_queues.lock().await;
        queues.insert(id, VecDeque::from(addresses));
    }

    /// Pop the next address from the queue for a given id
    pub async fn pop_next_addr(&self, id: u32) -> Option<Multiaddr> {
        let mut queues = self.dial_queues.lock().await;
        if let Some(queue) = queues.get_mut(&id) {
            queue.pop_front()
        } else {
            None
        }
    }

    /// Map a connection id to a dial sequence id
    pub async fn map_connid(&self, conn_id: ConnectionId, id: u32) {
        let mut map = self.conn_to_dial.lock().await;
        map.insert(conn_id, id);
    }

    /// Remove and get the dial sequence id for a connection id
    pub async fn remove_connid(&self, conn_id: &ConnectionId) -> Option<u32> {
        let mut map = self.conn_to_dial.lock().await;
        map.remove(conn_id)
    }

    /// Remove the queue for a dial sequence id
    pub async fn remove_queue(&self, id: u32) {
        let mut queues = self.dial_queues.lock().await;
        queues.remove(&id);
    }

    /// Returns the dial sequence ID associated with the given connection ID, if any.
    pub async fn get_dial_sequence_id_by_connection_id(
        &self,
        conn_id: &ConnectionId,
    ) -> Option<u32> {
        let map = self.conn_to_dial.lock().await;
        map.get(conn_id).cloned()
    }
}
