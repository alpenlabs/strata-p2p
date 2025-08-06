//! Score manager for the P2P network.

#[cfg_attr(
    not(any(feature = "gossipsub", feature = "request-response")),
    allow(unused_imports)
)]
use {libp2p::identity::PeerId, std::collections::HashMap};

/// Default application score for request-response protocol.
pub const DEFAULT_REQ_RESP_APP_SCORE: f64 = 0.0;

/// Default application score for gossipsub protocol.
pub const DEFAULT_GOSSIP_APP_SCORE: f64 = 0.0;

/// Manages peer scoring for different protocols in the P2P network.
///
/// Tracks application-level scores for peers across gossipsub and request-response protocols.
#[derive(Debug, Clone, Default)]
pub struct ScoreManager {
    /// [`HashMap`] of `gossipsub` app score for each peer.
    #[cfg(feature = "gossipsub")]
    gossipsub_app_score: HashMap<PeerId, f64>,

    /// [`HashMap`] of request-response score for each peer.
    #[cfg(feature = "request-response")]
    req_resp_app_score: HashMap<PeerId, f64>,
}

impl ScoreManager {
    /// Creates a new [`ScoreManager`] with empty score maps.
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "gossipsub")]
            gossipsub_app_score: HashMap::new(),
            #[cfg(feature = "request-response")]
            req_resp_app_score: HashMap::new(),
        }
    }

    /// Retrieves the gossipsub application score for a specific peer.
    ///
    /// Returns [`None`] if no score exists for the peer.
    #[cfg(feature = "gossipsub")]
    pub fn get_gossipsub_app_score(&self, peer_id: &PeerId) -> Option<f64> {
        self.gossipsub_app_score.get(peer_id).cloned()
    }

    /// Retrieves the request-response application score for a specific peer.
    ///
    /// Returns [`None`] if no score exists for the peer.
    #[cfg(feature = "request-response")]
    pub fn get_req_resp_score(&self, peer_id: &PeerId) -> Option<f64> {
        self.req_resp_app_score.get(peer_id).cloned()
    }

    /// Updates the gossipsub application score for a specific peer.
    #[cfg(feature = "gossipsub")]
    pub fn update_gossipsub_app_score(&mut self, peer_id: &PeerId, new_score: f64) {
        self.gossipsub_app_score.insert(peer_id.clone(), new_score);
    }

    /// Updates the request-response application score for a specific peer.
    #[cfg(feature = "request-response")]
    pub fn update_req_resp_app_score(&mut self, peer_id: &PeerId, new_score: f64) {
        self.req_resp_app_score.insert(peer_id.clone(), new_score);
    }
}

/// All scores for a peer.
#[cfg(any(feature = "gossipsub", feature = "request-response"))]
#[derive(Debug, Clone, Default)]
pub struct PeerScore {
    /// Gossipsub application score.
    #[cfg(feature = "gossipsub")]
    pub gossipsub_app_score: f64,

    /// Request-response application score.
    #[cfg(feature = "request-response")]
    pub req_resp_app_score: f64,

    /// Gossipsub internal score.
    #[cfg(feature = "gossipsub")]
    pub gossipsub_internal_score: f64,
}
