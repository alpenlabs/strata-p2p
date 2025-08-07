//! Request-Response [`Behaviour`] and [`NetworkBehaviour`] for the P2P protocol.

#![allow(
    missing_docs,
    reason = "avoid 'missing documentation for a variant' error from deriving `NetworkBehaviour`"
)]

#[cfg(feature = "gossipsub")]
use std::collections::HashSet;
use std::sync::Arc;

#[cfg(feature = "request-response")]
use libp2p::StreamProtocol;
#[cfg(feature = "request-response")]
use libp2p::request_response::{
    Behaviour as RequestResponse, Config as RequestResponseConfig, ProtocolSupport,
};
#[cfg(feature = "gossipsub")]
use libp2p::{
    PeerId,
    gossipsub::{
        self, Behaviour as Gossipsub, IdentityTransform, MessageAuthenticity, PeerScoreParams,
        PeerScoreThresholds, TopicScoreParams, ValidationMode, WhitelistSubscriptionFilter,
    },
};
use libp2p::{
    identify::{Behaviour as Identify, Config},
    swarm::NetworkBehaviour,
};

#[cfg(feature = "gossipsub")]
use super::MAX_TRANSMIT_SIZE;
#[cfg(feature = "gossipsub")]
use super::TOPIC;
#[cfg(feature = "request-response")]
use super::codec_raw;
#[cfg(feature = "byos")]
use crate::swarm::{PublicKey, setup::behavior::SetupBehaviour};
use crate::{signer::ApplicationSigner, swarm::Keypair};

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
/// - **Topic Subscription Filter**: Whitelisted to only allow subscription to the "bitvm2" topic
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
    gossipsub_score_params: &Option<PeerScoreParams>,
    gossipsub_score_thresholds: &Option<PeerScoreThresholds>,
) -> Result<Gossipsub<IdentityTransform, WhitelistSubscriptionFilter>, &'static str> {
    let mut filter = HashSet::new();
    filter.insert(TOPIC.hash()); // Target topic for subscription.

    let peer_score_params = gossipsub_score_params.clone().unwrap_or_else(|| {
        let mut params = PeerScoreParams::default();
        params.topics.insert(
            TOPIC.hash(),
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
            .max_transmit_size(MAX_TRANSMIT_SIZE)
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
    ///   - 512 KB max transmission size to prevent large message attacks
    ///   - `IDONTWANT` on publish to reduce duplicate forwarding overhead
    ///   - Whitelisted subscription filter for "bitvm2" topic only
    ///   - Configurable peer scoring system for network quality
    ///
    /// # Arguments
    ///
    /// * `protocol_name` - Protocol identifier used for request-response and identify protocols
    /// * `transport_keypair` - Transport keypair for network identity and message authentication
    /// * `app_public_key` - Application-level public key for setup behavior
    /// * `gossipsub_score_params` - Optional peer scoring parameters for gossipsub
    /// * `gossipsub_score_thresholds` - Optional peer scoring thresholds for gossipsub
    /// * `signer` - Application signer for setup behavior authentication
    /// # Returns
    ///
    /// Returns a configured [`Behaviour`] instance or an error string if configuration fails.
    pub fn new(
        protocol_name: &'static str,
        transport_keypair: &Keypair,
        #[cfg(feature = "byos")] app_public_key: &PublicKey,
        #[cfg(feature = "gossipsub")] gossipsub_score_params: &Option<PeerScoreParams>,
        #[cfg(feature = "gossipsub")] gossipsub_score_thresholds: &Option<PeerScoreThresholds>,
        #[cfg(feature = "byos")] signer: Arc<dyn ApplicationSigner>,
    ) -> Result<Self, &'static str> {
        #[cfg(feature = "gossipsub")]
        let gossipsub = create_gossipsub(
            transport_keypair,
            gossipsub_score_params,
            gossipsub_score_thresholds,
        )?;

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
            #[cfg(feature = "byos")]
            setup: SetupBehaviour::new(
                app_public_key.clone(),
                transport_keypair.public().to_peer_id(),
                signer,
            ),
        })
    }
}
