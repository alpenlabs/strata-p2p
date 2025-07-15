//! Request-Response [`Behaviour`] and [`NetworkBehaviour`] for the P2P protocol.

use std::collections::HashSet;

use libp2p::{
    PeerId, StreamProtocol,
    allow_block_list::{AllowedPeers, Behaviour as AllowListBehaviour},
    gossipsub::{
        self, Behaviour as Gossipsub, IdentityTransform, MessageAuthenticity,
        WhitelistSubscriptionFilter,
    },
    identify::{Behaviour as Identify, Config},
    identity::Keypair,
    request_response::{
        Behaviour as RequestResponse, Config as RequestResponseConfig, ProtocolSupport,
    },
    swarm::NetworkBehaviour,
};
use crate::swarm::setup::behavior::SetupBehaviour;
use super::{MAX_TRANSMIT_SIZE, TOPIC, codec_raw};

/// Alias for request-response behaviour with messages serialized by using
/// homebrewed codec implementation.
pub(crate) type RequestResponseRawBehaviour = RequestResponse<codec_raw::Codec>;

/// Composite behaviour which consists of other ones used by swarm in P2P
/// implementation.
#[expect(missing_debug_implementations)]
#[derive(NetworkBehaviour)]
pub struct Behaviour {
    /// Gossipsub - pub/sub model for messages distribution.
    pub gossipsub: Gossipsub<IdentityTransform, WhitelistSubscriptionFilter>,

    /// Identification of peers, address to connect to, public keys, etc.
    pub identify: Identify,

    /// Request-response model for recursive discovery of lost or skipped info.
    pub request_response: RequestResponseRawBehaviour,

    /// Connect only allowed peers by peer id.
    pub allow_list: AllowListBehaviour<AllowedPeers>,

    pub setup: SetupBehaviour,
}

impl Behaviour {
    /// Creates a new [`Behaviour`] given a `protocol_name`, [`Keypair`], and an allow list of
    /// [`PeerId`]s.
    pub fn new(protocol_name: &'static str, keypair: &Keypair, allowlist: &[PeerId]) -> Self {
        let mut allow_list = AllowListBehaviour::default();
        for peer in allowlist {
            allow_list.allow_peer(*peer);
        }

        let mut filter = HashSet::new();
        filter.insert(TOPIC.hash());

        Self {
            identify: Identify::new(Config::new(protocol_name.to_string(), keypair.public())),
            gossipsub: Gossipsub::new_with_subscription_filter(
                MessageAuthenticity::Author(PeerId::from_public_key(&keypair.public())),
                gossipsub::ConfigBuilder::default()
                    .validation_mode(gossipsub::ValidationMode::Permissive)
                    .validate_messages()
                    .max_transmit_size(MAX_TRANSMIT_SIZE)
                    // Avoids spamming the network and nodes with messages
                    .idontwant_on_publish(true)
                    .build()
                    .expect("gossipsub config at this stage must be valid"),
                None,
                WhitelistSubscriptionFilter(filter),
            )
            .unwrap(),
            request_response: RequestResponseRawBehaviour::new(
                [(StreamProtocol::new(protocol_name), ProtocolSupport::Full)],
                RequestResponseConfig::default(),
            ),
            allow_list,
            setup: SetupBehaviour::new(keypair.public().try_into_ed25519().unwrap())
        }
    }
}
