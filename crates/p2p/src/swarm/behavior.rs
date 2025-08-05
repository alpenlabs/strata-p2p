//! Request-Response [`Behaviour`] and [`NetworkBehaviour`] for the P2P protocol.

use std::collections::HashSet;
#[cfg(feature = "kad")]
use std::num::NonZero;

#[cfg(feature = "request-response")]
use libp2p::request_response::{
    Behaviour as RequestResponse, Config as RequestResponseConfig, ProtocolSupport,
};
use libp2p::{
    PeerId, StreamProtocol,
    gossipsub::{
        self, Behaviour as Gossipsub, IdentityTransform, MessageAuthenticity, Sha256Topic,
        WhitelistSubscriptionFilter,
    },
    identify::{Behaviour as Identify, Config},
    identity::{Keypair, PublicKey},
    swarm::NetworkBehaviour,
};
#[cfg(feature = "kad")]
use libp2p::{kad, kad::store::MemoryStore};

use super::MAX_TRANSMIT_SIZE;
#[cfg(feature = "request-response")]
use super::codec_raw;
#[cfg(feature = "kad")]
use crate::swarm::KadProtocol;
use crate::{
    signer::ApplicationSigner,
    swarm::{GossipSubTopic, setup::behavior::SetupBehaviour},
};

/// Alias for request-response behaviour with messages serialized by using
/// homebrewed codec implementation.
#[cfg(feature = "request-response")]
type RequestResponseRawBehaviour = RequestResponse<codec_raw::Codec>;

/// Composite behaviour which consists of other ones used by swarm in P2P
/// implementation.
#[expect(missing_debug_implementations)]
#[derive(NetworkBehaviour)]
pub struct Behaviour<S: ApplicationSigner> {
    /// Exchange application public keys before establish the connection.
    pub setup: SetupBehaviour<S>,

    /// Identification of peers, address to connect to, public keys, etc.
    pub identify: Identify,

    /// Request-response model for recursive discovery of lost or skipped info.
    #[cfg(feature = "request-response")]
    pub request_response: RequestResponseRawBehaviour,

    /// Gossipsub - pub/sub model for messages distribution.
    pub gossipsub: Gossipsub<IdentityTransform, WhitelistSubscriptionFilter>,

    /// Kademlia DHT
    #[cfg(feature = "kad")]
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
}

#[cfg(feature = "kad")]
fn configure_kademlia_behaviour(
    transport_keypair: &Keypair,
    kad_protocol_name: &Option<KadProtocol>,
) -> libp2p::kad::Behaviour<MemoryStore> {
    use crate::swarm::KadProtocol;

    let mut kad_cfg = kad::Config::new(
        kad_protocol_name
            .as_ref()
            .unwrap_or(&KadProtocol::V1)
            .clone()
            .into(),
    );

    // it is expected that there's going to be manual validation of records
    kad_cfg.set_record_filtering(kad::StoreInserts::FilterBoth);

    // it is expected that there's going to be manual filtering of peers based on their real
    // app_pk
    kad_cfg.set_kbucket_inserts(kad::BucketInserts::Manual);

    // TODO(Arniiiii): make it configurable
    kad_cfg.set_replication_factor(NonZero::new(5).unwrap());

    // maybe should be increased and give logic of quorum manually
    kad_cfg.set_caching(kad::Caching::Enabled { max_peers: 1 });

    let store = kad::store::MemoryStore::new(transport_keypair.public().to_peer_id());

    let mut kademlia_behaviour =
        kad::Behaviour::with_config(transport_keypair.public().to_peer_id(), store, kad_cfg);

    // Enable server mode for DHT
    kademlia_behaviour.set_mode(Some(kad::Mode::Server));

    kademlia_behaviour
}

impl<S: ApplicationSigner> Behaviour<S> {
    /// Creates a new [`Behaviour`] given a `protocol_name`, transport [`Keypair`], app
    /// [`PublicKey`], signer, and an allow list of [`PeerId`]s.
    pub fn new(
        protocol_name: &'static str,
        transport_keypair: &Keypair,
        app_public_key: &PublicKey,
        signer: S,
        #[cfg(feature = "kad")] kad_protocol_name: &Option<KadProtocol>,
    ) -> Self {
        let mut filter = HashSet::new();
        filter.insert(Into::<Sha256Topic>::into(GossipSubTopic::V2).hash());

        #[cfg(feature = "kad")]
        let kademlia_behaviour = configure_kademlia_behaviour(transport_keypair, kad_protocol_name);

        Self {
            identify: Identify::new(Config::new(
                protocol_name.to_string(),
                transport_keypair.public(),
            )),
            gossipsub: Gossipsub::new_with_subscription_filter(
                MessageAuthenticity::Author(PeerId::from_public_key(&transport_keypair.public())),
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
            #[cfg(feature = "request-response")]
            request_response: RequestResponseRawBehaviour::new(
                [(StreamProtocol::new(protocol_name), ProtocolSupport::Full)],
                RequestResponseConfig::default(),
            ),
            #[cfg(feature = "kad")]
            kademlia: kademlia_behaviour,
            setup: SetupBehaviour::new(
                app_public_key.clone(),
                transport_keypair.public().to_peer_id(),
                signer,
            ),
        }
    }
}
