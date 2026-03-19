//! Events emitted from P2P.

#[cfg(feature = "kad")]
use libp2p::Multiaddr;
#[cfg(feature = "gossipsub")]
use libp2p::identity::PublicKey;
/// Events emitted from P2P that needs to be handled by the user.
#[cfg(feature = "request-response")]
use tokio::sync::oneshot::Sender;

/// Authenticated gossipsub message delivered to the peer.
#[cfg(feature = "gossipsub")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedGossipMessage {
    /// Authenticated sender key from the signed gossipsub message.
    pub sender: PublicKey,
    /// Message bytes carried by the gossipsub message.
    pub data: Vec<u8>,
}

/// Events emitted from the gossipsub protocol.
#[cfg(feature = "gossipsub")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GossipEvent {
    /// Authenticated gossipsub message delivered to the peer.
    ReceivedMessage(ReceivedGossipMessage),
}

/// Events emitted from the request/response protocol.
#[cfg(feature = "request-response")]
#[derive(Debug)]
pub enum ReqRespEvent {
    /// Received a request from other peer.
    ReceivedRequest(Vec<u8>, Sender<Vec<u8>>),

    /// Received a response from other peer.
    ReceivedResponse(Vec<u8>),
}

/// Events emitted from the command handler.
#[derive(Debug, Clone)]
pub enum CommandEvent {
    /// Result of `FindMultiaddress`
    #[cfg(feature = "kad")]
    ResultFindMultiaddress(Option<Vec<Multiaddr>>),
}
