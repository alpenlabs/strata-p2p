//! Events emitted from P2P.

/// Events emitted from P2P that needs to be handled by the user.

#[derive(Debug, Clone)]
pub enum Event {
    /// Received message from other peer.
    ReceivedMessage(Vec<u8>),

    /// Received a request from other peer.
    ReceivedRequest(Vec<u8>),
}
