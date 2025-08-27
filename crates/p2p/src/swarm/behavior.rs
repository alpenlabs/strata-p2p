//! Request-Response [`Behaviour`] and [`NetworkBehaviour`] for the P2P protocol.

#![allow(
    missing_docs,
    reason = "avoid 'missing documentation for a variant' error from deriving `NetworkBehaviour`"
)]

#[cfg(feature = "gossipsub")]
use std::collections::HashSet;
#[cfg(feature = "byos")]
use std::sync::Arc;

#[cfg(feature = "request-response")]
use libp2p::StreamProtocol;
#[cfg(any(feature = "mem-conn-limits-abs", feature = "mem-conn-limits-rel"))]
use libp2p::memory_connection_limits::Behaviour as MemConnLimitsBehavior;
#[cfg(feature = "request-response")]
use libp2p::request_response::{
    Behaviour as RequestResponse, Config as RequestResponseConfig, ProtocolSupport,
};
#[cfg(feature = "gossipsub")]
use libp2p::{
    PeerId,
    gossipsub::{
        self, Behaviour as Gossipsub, IdentityTransform, MessageAuthenticity, PeerScoreParams,
        PeerScoreThresholds, Sha256Topic, TopicScoreParams, ValidationMode,
        WhitelistSubscriptionFilter,
    },
};
use libp2p::{
    connection_limits::{Behaviour as ConnectionLimitsBehaviour, ConnectionLimits},
    identify::{Behaviour as Identify, Config},
    swarm::NetworkBehaviour,
};
#[cfg(feature = "kad")]
use libp2p::{kad, kad::store::MemoryStore};

#[cfg(feature = "request-response")]
use super::codec_raw;
#[cfg(feature = "byos")]
use crate::signer::ApplicationSigner;
#[cfg(feature = "kad")]
use crate::swarm::KadProtocol;
use crate::swarm::Keypair;
#[cfg(feature = "byos")]
use crate::swarm::{PublicKey, setup::behavior::SetupBehaviour};

/// Alias for request-response behaviour with messages serialized by using
/// homebrewed codec implementation.
#[cfg(feature = "request-response")]
pub(crate) type RequestResponseRawBehaviour = RequestResponse<codec_raw::Codec>;

/// Composite behaviour which consists of other ones used by swarm in P2P
/// implementation.
#[expect(missing_debug_implementations)]
#[derive(NetworkBehaviour)]
pub struct Behaviour {
    /// Exchange application public keys before establish the connection.
    /// Only used when BYOS (Bring Your Own Signer) feature is enabled.
    #[cfg(feature = "byos")]
    pub setup: SetupBehaviour,

    /// Identification of peers, address to connect to, public keys, etc.
    pub identify: Identify,

    /// Request-response model for recursive discovery of lost or skipped info.
    #[cfg(feature = "request-response")]
    pub request_response: RequestResponseRawBehaviour,

    /// Gossipsub - pub/sub model for messages distribution.
    #[cfg(feature = "gossipsub")]
    pub gossipsub: Gossipsub<IdentityTransform, WhitelistSubscriptionFilter>,

    /// Kademlia DHT
    #[cfg(feature = "kad")]
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,

    /// Limits amount of connection.
    pub conn_limits: ConnectionLimitsBehaviour,

    /// Denies new connection when used RAM by the process is above a specified amount of bytes.
    #[cfg(feature = "mem-conn-limits-abs")]
    pub mem_conn_limits_abs: MemConnLimitsBehavior,

    /// Denies new connection when used RAM by the process is above a specified percentage of RAM.
    #[cfg(feature = "mem-conn-limits-rel")]
    pub mem_conn_limits_rel: MemConnLimitsBehavior,
}

/// Creates a new [`Gossipsub`] given a [`Keypair`] and scoring parameters.
///
/// # [`Gossipsub`] implementation details
///
/// Under the hood the following [`Gossipsub`] features are implemented:
///
/// - **Message Validation**: All messages are validated using
///   [permissive](ValidationMode::Permissive) validation mode
/// - **Message Authentication**: Uses author-based authentication with the peer's public key
/// - **Max Transmission Size**: Limited to 512 KB (524,288 bytes) to prevent large message attacks
/// - **`IDONTWANT` on Publish**: Enabled to reduce duplicate message forwarding overhead
/// - **Topic Subscription Filter**: Whitelisted to only allow subscription to the configured topic
/// - **Peer Scoring**: Configurable peer scoring system to maintain network quality
/// - **Default Scoring**: If no custom scoring params provided, uses default params with the target
///   topic
///
/// # Arguments
///
/// * `keypair` - Transport keypair used for message authentication
/// * `gossipsub_score_params` - Optional peer scoring parameters for network quality management
/// * `gossipsub_score_thresholds` - Optional peer scoring thresholds for peer evaluation
///
/// # Returns
///
/// Returns a configured [`Gossipsub`] instance or an error string if configuration fails.
#[cfg(feature = "gossipsub")]
fn create_gossipsub(
    keypair: &Keypair,
    topic: &Sha256Topic,
    gossipsub_score_params: &Option<PeerScoreParams>,
    gossipsub_score_thresholds: &Option<PeerScoreThresholds>,
    max_transmit_size: usize,
) -> Result<Gossipsub<IdentityTransform, WhitelistSubscriptionFilter>, &'static str> {
    let mut filter = HashSet::new();
    filter.insert(topic.hash()); // Target topic for subscription.

    let peer_score_params = gossipsub_score_params.clone().unwrap_or_else(|| {
        let mut params = PeerScoreParams::default();
        params.topics.insert(
            topic.hash(),
            TopicScoreParams {
                ..Default::default()
            },
        );
        params
    });

    let peer_score_thresholds = gossipsub_score_thresholds.clone().unwrap_or_default();

    let gossipsub = Gossipsub::new_with_subscription_filter(
        MessageAuthenticity::Author(PeerId::from_public_key(&keypair.public().clone())),
        gossipsub::ConfigBuilder::default()
            .validation_mode(ValidationMode::Permissive)
            .validate_messages()
            .max_transmit_size(max_transmit_size)
            .idontwant_on_publish(true)
            .build()
            .expect("gossipsub config at this stage must be valid"),
        None,
        WhitelistSubscriptionFilter(filter),
    );

    match gossipsub {
        Ok(mut gossipsub) => {
            let _ = gossipsub.with_peer_score(peer_score_params, peer_score_thresholds);
            Ok(gossipsub)
        }
        Err(e) => Err(e),
    }
}

/// Here we create kademlia behaviour.
#[cfg(feature = "kad")]
fn create_kademlia_behaviour(
    transport_keypair: &Keypair,
    kad_protocol_name: &Option<KadProtocol>,
) -> libp2p::kad::Behaviour<MemoryStore> {
    let mut kad_cfg = kad::Config::new(
        kad_protocol_name
            .as_ref()
            .unwrap_or(&KadProtocol::V1)
            .clone()
            .into(),
    );

    // it is expected that there's going to be manual validation of records
    kad_cfg.set_record_filtering(kad::StoreInserts::FilterBoth);

    // it is expected that there's going to be automatic filtering of peers based on their real
    // app_pk if feature="byos" when we received `SetupBehaviourEvent::AppKeyReceived` and if not
    // feature="byos" , then filter based on transport id at handling
    // FromSwarm::ConnectionEstablished
    kad_cfg.set_kbucket_inserts(kad::BucketInserts::OnConnected);

    let store = kad::store::MemoryStore::new(transport_keypair.public().to_peer_id());

    let mut kademlia_behaviour =
        kad::Behaviour::with_config(transport_keypair.public().to_peer_id(), store, kad_cfg);

    // Enable server mode for DHT
    kademlia_behaviour.set_mode(Some(kad::Mode::Server));

    kademlia_behaviour
}

impl Behaviour {
    /// Creates a new [`Behaviour`] with all configured sub-behaviors for P2P networking.
    ///
    /// # Implementation details
    ///
    /// This constructs a composite behavior that includes:
    ///
    /// - **Setup Behavior**: Exchanges application public keys before establishing connections
    /// - **Identify Protocol**: Peer identification, address discovery, and public key exchange
    /// - **Request-Response**: Recursive discovery protocol for lost or skipped information
    /// - **Gossipsub**: Pub/sub message distribution with the following features:
    ///   - [Permissive](ValidationMode::Permissive) message validation with author-based
    ///     authentication
    ///   - Configurable max transmission size (default 512 KB) to prevent large message attacks
    ///   - `IDONTWANT` on publish to reduce duplicate forwarding overhead
    ///   - Whitelisted subscription filter for the configured topic only
    ///   - Configurable peer scoring system for network quality
    ///
    /// # Arguments
    ///
    /// * `protocol_name` - Protocol identifier used for request-response and identify protocols;
    ///   typically provided via P2PConfig
    /// * `transport_keypair` - Transport keypair for network identity and message authentication
    /// * `app_public_key` - Application-level public key for setup behavior
    /// * `gossipsub_score_params` - Optional peer scoring parameters for gossipsub
    /// * `gossipsub_score_thresholds` - Optional peer scoring thresholds for gossipsub
    /// * `signer` - Application signer for setup behavior authentication
    /// # Returns
    ///
    /// Returns a configured [`Behaviour`] instance or an error string if configuration fails.
    #[allow(
        clippy::too_many_arguments,
        reason = "This is a composite behaviour with multiple sub-behaviours"
    )]
    pub fn new(
        protocol_name: &'static str,
        transport_keypair: &Keypair,
        #[cfg(feature = "byos")] app_public_key: &PublicKey,
        #[cfg(feature = "gossipsub")] topic: &Sha256Topic,
        #[cfg(feature = "gossipsub")] gossipsub_score_params: &Option<PeerScoreParams>,
        #[cfg(feature = "gossipsub")] gossipsub_score_thresholds: &Option<PeerScoreThresholds>,
        #[cfg(feature = "gossipsub")] gossipsub_max_transmit_size: usize,
        #[cfg(feature = "byos")] signer: Arc<dyn ApplicationSigner>,
        #[cfg(feature = "kad")] kad_protocol_name: &Option<KadProtocol>,
        connection_limits: ConnectionLimits,
        #[cfg(feature = "mem-conn-limits-abs")] max_allowed_bytes: usize,
        #[cfg(feature = "mem-conn-limits-rel")] max_percentage: f64,
    ) -> Result<Self, &'static str> {
        #[cfg(feature = "gossipsub")]
        let gossipsub = create_gossipsub(
            transport_keypair,
            topic,
            gossipsub_score_params,
            gossipsub_score_thresholds,
            gossipsub_max_transmit_size,
        )?;

        #[cfg(feature = "kad")]
        let kademlia_behaviour = create_kademlia_behaviour(transport_keypair, kad_protocol_name);

        Ok(Self {
            identify: Identify::new(Config::new(
                protocol_name.to_string(),
                transport_keypair.public(),
            )),
            #[cfg(feature = "gossipsub")]
            gossipsub,
            #[cfg(feature = "request-response")]
            request_response: RequestResponseRawBehaviour::new(
                [(StreamProtocol::new(protocol_name), ProtocolSupport::Full)],
                RequestResponseConfig::default(),
            ),
            #[cfg(feature = "kad")]
            kademlia: kademlia_behaviour,
            #[cfg(feature = "byos")]
            setup: SetupBehaviour::new(
                app_public_key.clone(),
                transport_keypair.public().to_peer_id(),
                signer,
            ),
            conn_limits: ConnectionLimitsBehaviour::new(connection_limits),
            #[cfg(feature = "mem-conn-limits-abs")]
            mem_conn_limits_abs: MemConnLimitsBehavior::with_max_bytes(max_allowed_bytes),
            #[cfg(feature = "mem-conn-limits-rel")]
            mem_conn_limits_rel: MemConnLimitsBehavior::with_max_percentage(max_percentage),
        })
    }
}
