//! Events emitted from P2P.

/// Events emitted from P2P that needs to be handled by the user.
use tokio::sync::oneshot::Sender;

#[derive(Debug, Clone)]
/// Events emitted from the gossipsub protocol.
pub enum GossipEvent {
    /// Received message from other peer.
    ReceivedMessage(Vec<u8>),
}

#[derive(Debug)]
/// Events emitted from the request/response protocol.
pub enum ReqRespEvent {
    /// Received a request from other peer.
    ReceivedRequest(Vec<u8>, Sender<Vec<u8>>),
    /// Received a response from other peer
    ReceivedResponse(Vec<u8>),
}
