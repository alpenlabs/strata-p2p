//! Request-Response [`Behaviour`] and [`NetworkBehaviour`] for the P2P protocol.

use std::collections::HashSet;

use libp2p::{
    PeerId, StreamProtocol,
    allow_block_list::{Behaviour as AllowListBehaviour, BlockedPeers},
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
    pub blacklist_behaviour: AllowListBehaviour<BlockedPeers>,
}

impl Behaviour {
    /// Creates a new [`Behaviour`] given a `protocol_name`, [`Keypair`], and a block list of
    /// [`PeerId`]s.
    pub fn new(protocol_name: &'static str, keypair: &Keypair, blacklist: &[PeerId]) -> Self {
        let mut blacklist_behaviour = AllowListBehaviour::default();
        for peer in blacklist {
            blacklist_behaviour.block_peer(*peer);
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
            blacklist_behaviour,
        }
    }
}
