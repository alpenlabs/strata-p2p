//! Request-Response [`Behaviour`] and [`NetworkBehaviour`] for the P2P protocol.

use std::collections::HashSet;

use libp2p::{
    PeerId, StreamProtocol,
    gossipsub::{
        self, Behaviour as Gossipsub, IdentityTransform, MessageAuthenticity,
        WhitelistSubscriptionFilter,
    },
    identify::{Behaviour as Identify, Config},
    identity::Keypair,
    kad,
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

    /// Kademlia DHT
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
}

impl Behaviour {
    /// Creates a new [`Behaviour`] given a `protocol_name`, [`Keypair`], and a block list of
    /// [`PeerId`]s.
    pub fn new(
        protocol_name: &'static str,
        keypair: &Keypair,
        kad_protocol_name: StreamProtocol,
    ) -> Self {
        let mut filter = HashSet::new();
        filter.insert(TOPIC.hash());

        let mut kad_cfg = kad::Config::new(kad_protocol_name);

        // it is expected that there's going to be manual validation of records
        kad_cfg.set_record_filtering(kad::StoreInserts::FilterBoth);

        // it is expected that there's going to be manual filtering of peers based on their real
        // app_pk
        kad_cfg.set_kbucket_inserts(kad::BucketInserts::Manual);

        // maybe should be increased and give logic of quorum manually
        kad_cfg.set_caching(kad::Caching::Enabled { max_peers: 1 });

        let store = kad::store::MemoryStore::new(keypair.public().to_peer_id());

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
            kademlia: kad::Behaviour::with_config(keypair.public().to_peer_id(), store, kad_cfg),
        }
    }
}
