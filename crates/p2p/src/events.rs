use bitcoin::hashes::sha256;
use libp2p::PeerId;
use prost::Message;
use strata_p2p_wire::p2p::v1::{GossipsubMsg, GossipsubMsgKind};

/// Events emitted from P2P to handle from operator side.
///
/// `DepositSetupPayload` is generic as some implementation details for different BitVM2
/// applications may vary. The only requirement for them to be decodable from bytes as protobuf
/// message.
#[derive(Clone, Debug)]
pub struct Event<DepositSetupPayload: Message + Clone> {
    pub peer_id: PeerId,
    pub kind: EventKind<DepositSetupPayload>,
}

impl<DSP: Message + Clone> Event<DSP> {
    pub fn new(peer_id: PeerId, kind: EventKind<DSP>) -> Self {
        Self { peer_id, kind }
    }

    pub fn scope(&self) -> Option<sha256::Hash> {
        // TODD(Velnbur): when other tpes of event are added, remove this one:
        #[allow(irrefutable_let_patterns)]
        let EventKind::GossipsubMsg(GossipsubMsg { kind, .. }) = &self.kind
        else {
            return None;
        };

        match kind {
            GossipsubMsgKind::Deposit { scope, .. } => Some(*scope),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum EventKind<DepositSetupPayload: Message + Clone> {
    GossipsubMsg(GossipsubMsg<DepositSetupPayload>),
}

impl<DepositSetupPayload: Message + Clone> From<GossipsubMsg<DepositSetupPayload>>
    for EventKind<DepositSetupPayload>
{
    fn from(v: GossipsubMsg<DepositSetupPayload>) -> Self {
        Self::GossipsubMsg(v)
    }
}
