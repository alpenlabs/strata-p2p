//! Request-Response [`Behaviour`] and [`NetworkBehaviour`] for the P2P protocol.

#![allow(
    missing_docs,
    reason = "avoid 'missing documentation for a variant' error from deriving `NetworkBehaviour`"
)]

#[cfg(feature = "gossipsub")]
use std::collections::HashSet;

#[cfg(feature = "request-response")]
use libp2p::request_response::{
    Behaviour as RequestResponse, Config as RequestResponseConfig, ProtocolSupport,
};
#[cfg(feature = "request-response")]
use libp2p::StreamProtocol;
#[cfg(feature = "gossipsub")]
use libp2p::{
    gossipsub::{
        self, Behaviour as Gossipsub, IdentityTransform, MessageAuthenticity, PeerScoreParams,
        PeerScoreThresholds, TopicScoreParams, WhitelistSubscriptionFilter,
    },
    PeerId,
};
use libp2p::{
    identify::{Behaviour as Identify, Config},
    swarm::NetworkBehaviour,
};

#[cfg(feature = "request-response")]
use super::codec_raw;
#[cfg(feature = "gossipsub")]
use super::MAX_TRANSMIT_SIZE;
#[cfg(feature = "gossipsub")]
use super::TOPIC;
use crate::{
    signer::ApplicationSigner,
    swarm::{setup::behavior::SetupBehaviour, Keypair, PublicKey},
};

/// Alias for request-response behaviour with messages serialized by using
/// homebrewed codec implementation.
#[cfg(feature = "request-response")]
pub(crate) type RequestResponseRawBehaviour = RequestResponse<codec_raw::Codec>;

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
    #[cfg(feature = "gossipsub")]
    pub gossipsub: Gossipsub<IdentityTransform, WhitelistSubscriptionFilter>,
}

#[cfg(feature = "gossipsub")]
fn create_gossipsub(
    keypair: &Keypair,
    gossipsub_score_params: &Option<PeerScoreParams>,
    gossipsub_score_thresholds: &Option<PeerScoreThresholds>,
) -> Gossipsub<IdentityTransform, WhitelistSubscriptionFilter> {
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

    let mut gossipsub = Gossipsub::new_with_subscription_filter(
        MessageAuthenticity::Author(PeerId::from_public_key(&keypair.public().clone())),
        gossipsub::ConfigBuilder::default()
            .validation_mode(gossipsub::ValidationMode::Permissive)
            .validate_messages()
            .max_transmit_size(MAX_TRANSMIT_SIZE)
            .idontwant_on_publish(true)
            .build()
            .expect("gossipsub config at this stage must be valid"),
        None,
        WhitelistSubscriptionFilter(filter),
    )
    .unwrap();

    // Apply peer score configuration
    let _ = gossipsub.with_peer_score(peer_score_params, peer_score_thresholds);

    gossipsub
}

impl<S: ApplicationSigner> Behaviour<S> {
    /// Creates a new [`Behaviour`] given a `protocol_name`, transport [`Keypair`], app
    /// [`PublicKey`], signer, and an allow list of [`PeerId`]s.
    pub fn new(
        protocol_name: &'static str,
        transport_keypair: &Keypair,
        app_public_key: &PublicKey,
        #[cfg(feature = "gossipsub")] gossipsub_score_params: &Option<PeerScoreParams>,
        #[cfg(feature = "gossipsub")] gossipsub_score_thresholds: &Option<PeerScoreThresholds>,
        signer: S,
    ) -> Self {
        #[cfg(feature = "gossipsub")]
        let gossipsub = create_gossipsub(
            transport_keypair,
            gossipsub_score_params,
            gossipsub_score_thresholds,
        );

        Self {
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
            setup: SetupBehaviour::new(
                app_public_key.clone(),
                transport_keypair.public().to_peer_id(),
                signer,
            ),
        }
    }
}
