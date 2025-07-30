//! Request-Response [`Behaviour`] and [`NetworkBehaviour`] for the P2P protocol.

#![allow(
    missing_docs,
    reason = "avoid 'missing documentation for a variant' error from deriving `NetworkBehaviour`"
)]

use std::{collections::HashSet, time::Duration};

use libp2p::{
    allow_block_list::{AllowedPeers, Behaviour as AllowListBehaviour},
    gossipsub::{
        self, Behaviour as Gossipsub, IdentityTransform, MessageAuthenticity, PeerScoreParams,
        PeerScoreThresholds, TopicScoreParams, WhitelistSubscriptionFilter,
    },
    identify::{Behaviour as Identify, Config},
    identity::Keypair,
    request_response::{
        Behaviour as RequestResponse, Config as RequestResponseConfig, ProtocolSupport,
    },
    swarm::NetworkBehaviour,
    PeerId, StreamProtocol,
};

use super::{codec_raw, MAX_TRANSMIT_SIZE, TOPIC};
use crate::{signer::ApplicationSigner, swarm::setup::behavior::SetupBehaviour};

/// Alias for request-response behaviour with messages serialized by using
/// homebrewed codec implementation.
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
    pub request_response: RequestResponseRawBehaviour,

    /// Gossipsub - pub/sub model for messages distribution.
    pub gossipsub: Gossipsub<IdentityTransform, WhitelistSubscriptionFilter>,
}

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
        gossipsub_score_params: &Option<PeerScoreParams>,
        gossipsub_score_thresholds: &Option<PeerScoreThresholds>,
        signer: S,
    ) -> Self {
        let mut filter = HashSet::new();
        filter.insert(TOPIC.hash());

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
            gossipsub,
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
