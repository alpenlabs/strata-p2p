//! Request-Response [`Behaviour`] and [`NetworkBehaviour`] for the P2P protocol.

use std::{collections::HashSet, time::Duration};

use bitcoin::hashes::{sha256, Hash};
use libp2p::{
    allow_block_list::{AllowedPeers, Behaviour as AllowListBehaviour},
    gossipsub::{
        self, Behaviour as Gossipsub, IdentityTransform, MessageAuthenticity, MessageId,
        WhitelistSubscriptionFilter,
    },
    identify::{Behaviour as Identify, Config},
    identity::{secp256k1::Keypair, PublicKey},
    request_response::{
        Behaviour as RequestResponse, Config as RequestResponseConfig, ProtocolSupport,
    },
    swarm::NetworkBehaviour,
    PeerId, StreamProtocol,
};
use strata_p2p_wire::p2p::v1::proto::{GetMessageRequest, GetMessageResponse};

use super::{codec, MAX_TRANSMIT_SIZE, TOPIC};

/// Alias for request-response behaviour with messages serialized by using
/// homebrewed codec implementation.
pub(crate) type RequestResponseProtoBehaviour<Req, Resp> = RequestResponse<codec::Codec<Req, Resp>>;

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
    pub request_response: RequestResponseProtoBehaviour<GetMessageRequest, GetMessageResponse>,

    /// Connect only allowed peers by peer id.
    pub allow_list: AllowListBehaviour<AllowedPeers>,
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
                    .validate_messages()
                    .max_transmit_size(MAX_TRANSMIT_SIZE)
                    // Avoids spamming the network and nodes with messages
                    .duplicate_cache_time(Duration::from_secs(60 * 5)) // default is 1 min
                    .published_message_ids_cache_time(Duration::from_secs(60)) // default is 10
                    .max_ihave_messages(100) // default is 10
                    .gossip_retransimission(1) // default is 3
                    .iwant_followup_time(Duration::from_secs(2)) // default is 3
                    .gossip_lazy(2) // default is 6
                    .gossip_factor(0.1) // default is 0.25
                    .history_gossip(1) // default is 3
                    .history_length(3) // default is 5
                    // We want a unique message id for each message, so we use the hash of the
                    // message data instead of the default one, that is the concatenation of the
                    // PeerId and the sequence number of the message.
                    //
                    // NOTE(@storopoli): I don't trust the default one, since we are not using the
                    //                   LibP2P's Message template, hence the sequence number might
                    //                   not exist, and in that case it is always be set to 0 by
                    //                   default.
                    .message_id_fn(|msg| {
                        let hash = sha256::Hash::hash(msg.data.as_ref());
                        MessageId::new(hash.as_ref())
                    })
                    .build()
                    .expect("gossipsub config at this stage must be valid"),
                None,
                WhitelistSubscriptionFilter(filter),
            )
            .unwrap(),
            request_response: RequestResponseProtoBehaviour::new(
                [(StreamProtocol::new(protocol_name), ProtocolSupport::Full)],
                RequestResponseConfig::default(),
            ),
            allow_list,
        }
    }
}
