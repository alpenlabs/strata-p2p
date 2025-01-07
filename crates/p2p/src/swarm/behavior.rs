use std::collections::HashSet;

use libp2p::{
    allow_block_list::{AllowedPeers, Behaviour as AllowListBehaviour},
    gossipsub::{
        self, Behaviour as Gossipsub, Hasher, IdentityTransform, MessageAuthenticity,
        WhitelistSubscriptionFilter,
    },
    identify::{Behaviour as Identify, Config},
    identity::{secp256k1::Keypair, PublicKey},
    ping::Behaviour as Ping,
    request_response::{
        Behaviour as RequestResponse, Config as RequestResponseConfig, ProtocolSupport,
    },
    swarm::NetworkBehaviour,
    PeerId, StreamProtocol,
};

use super::{codec, hasher::Sha256Hasher, TOPIC};
use crate::wire::p2p::v1::{GetMessageRequest, GetMessageResponse};

pub type RequestResponseProtoBehaviour<Req, Resp> = RequestResponse<codec::Codec<Req, Resp>>;

#[derive(NetworkBehaviour)]
pub struct Behaviour {
    pub gossipsub: Gossipsub<IdentityTransform, WhitelistSubscriptionFilter>,
    pub ping: Ping,
    pub identify: Identify,
    pub request_response: RequestResponseProtoBehaviour<GetMessageRequest, GetMessageResponse>,
    pub allow_list: AllowListBehaviour<AllowedPeers>,
}

impl Behaviour {
    pub fn new(protocol_name: &'static str, keypair: &Keypair, allowlist: &[PeerId]) -> Self {
        let mut allow_list = AllowListBehaviour::default();
        for peer in allowlist {
            allow_list.allow_peer(*peer);
        }

        let mut filter = HashSet::new();
        filter.insert(Sha256Hasher::hash(TOPIC.to_owned()));

        Self {
            identify: Identify::new(Config::new(
                protocol_name.to_string(),
                PublicKey::from(keypair.public().clone()),
            )),
            gossipsub: Gossipsub::new_with_subscription_filter(
                MessageAuthenticity::Author(PeerId::from_public_key(
                    &libp2p::identity::PublicKey::from(keypair.public().clone()),
                )),
                gossipsub::ConfigBuilder::default()
                    .validation_mode(gossipsub::ValidationMode::Permissive)
                    .flood_publish(true)
                    .mesh_n_low(1)
                    .mesh_outbound_min(1)
                    .validate_messages()
                    .build()
                    .expect("gossipsub config at this stage must be valid"),
                None,
                WhitelistSubscriptionFilter(filter),
            )
            .unwrap(),
            ping: Ping::default(),
            request_response: RequestResponseProtoBehaviour::new(
                [(StreamProtocol::new(protocol_name), ProtocolSupport::Full)],
                RequestResponseConfig::default(),
            ),
            allow_list,
        }
    }
}
