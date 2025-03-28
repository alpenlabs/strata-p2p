//! Events emitted from P2P.

use strata_p2p_wire::p2p::v1::{GossipsubMsg, GetMessageRequest};

/// Events emitted from P2P to handle from operator side.
///
/// # Implementation Details
///
/// `DepositSetupPayload` is generic as some implementation details for different BitVM2
/// applications may vary. The only requirement for them to be decodable from bytes as protobuf
/// message.
#[derive(Debug, Clone)]
#[expect(clippy::large_enum_variant)]
pub enum Event {
    /// Received message from other operator.
    ReceivedMessage(GossipsubMsg),

    /// Received a request from other operator.
    ReceivedRequest(GetMessageRequest),
}

impl From<GossipsubMsg> for Event {
    fn from(v: GossipsubMsg) -> Self {
        Self::ReceivedMessage(v)
    }
}
