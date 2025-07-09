//! Events emitted from P2P.

/// Events emitted from P2P that needs to be handled by the user.
use tokio::sync::oneshot::Sender;

/// Events emitted from the gossipsub protocol.

#[derive(Debug, Clone)]
pub enum GossipEvent {
    /// Received message from other peer.
    ReceivedMessage(Vec<u8>),
}

/// Events emitted from the request/response protocol.
#[derive(Debug)]
pub enum ReqRespEvent {
    /// Received a request from other peer.
    ReceivedRequest(Vec<u8>, Sender<Vec<u8>>),
    /// Received a response from other peer
    ReceivedResponse(Vec<u8>),
}
