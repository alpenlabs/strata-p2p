//! Events emitted from P2P.

use strata_p2p_wire::p2p::v1::GossipsubMsg;

/// Events emitted from P2P to handle from operator side.
///
/// # Implementation Details
///
/// `DepositSetupPayload` is generic as some implementation details for different BitVM2
/// applications may vary. The only requirement for them to be decodable from bytes as protobuf
/// message.
#[derive(Clone, Debug)]
pub enum Event {
    /// Received message from other operator.
    ReceivedMessage(GossipsubMsg),
}

impl From<GossipsubMsg> for Event {
    fn from(v: GossipsubMsg) -> Self {
        Self::ReceivedMessage(v)
    }
}
