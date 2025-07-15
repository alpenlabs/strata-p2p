//! Events emitted from P2P.

/// Events emitted from P2P that needs to be handled by the user.
#[cfg(feature = "request-response")]
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
    #[cfg(feature = "request-response")]
    ReceivedRequest(Vec<u8>, Sender<Vec<u8>>),

    /// Received a request from another peer.
    ///
    /// NOTE: Even when disabled we still need a request-response mechanism to implement the
    /// setup phase.
    #[cfg(not(feature = "request-response"))]
    ReceivedRequest(Vec<u8>),

    /// Received a response from other peer.
    #[cfg(feature = "request-response")]
    ReceivedResponse(Vec<u8>),

    /// Received a response from another peer.
    ///
    /// NOTE: Even when disabled we still need a request-response mechanism to implement the
    /// setup phase.
    #[cfg(not(feature = "request-response"))]
    ReceivedResponse,
}
