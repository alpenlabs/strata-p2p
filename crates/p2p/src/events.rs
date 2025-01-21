use prost::Message;
use strata_p2p_wire::p2p::v1::GossipsubMsg;

/// Events emitted from P2P to handle from operator side.
///
/// `DepositSetupPayload` is generic as some implementation details for different BitVM2
/// applications may vary. The only requirement for them to be decodable from bytes as protobuf
/// message.
#[derive(Clone, Debug)]
pub enum Event<DepositSetupPayload: Message + Clone> {
    ReceivedMessage(GossipsubMsg<DepositSetupPayload>),
}

impl<DepositSetupPayload: Message + Clone> From<GossipsubMsg<DepositSetupPayload>>
    for Event<DepositSetupPayload>
{
    fn from(v: GossipsubMsg<DepositSetupPayload>) -> Self {
        Self::ReceivedMessage(v)
    }
}
