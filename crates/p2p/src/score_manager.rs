//! Score manager for the P2P network.

/// Default application score for request-response protocol.
#[cfg(feature = "request-response")]
pub const DEFAULT_REQ_RESP_APP_SCORE: f64 = 0.0;

/// Default application score for gossipsub protocol.
#[cfg(feature = "gossipsub")]
pub const DEFAULT_GOSSIP_APP_SCORE: f64 = 0.0;

#[cfg(any(feature = "gossipsub", feature = "request-response"))]
use std::collections::HashMap;
#[cfg(any(feature = "gossipsub", feature = "request-response"))]
use std::time::SystemTime;

#[cfg(any(feature = "gossipsub", feature = "request-response"))]
use libp2p::identity::PeerId;

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

    /// Storage for last scoring decay time.
    #[cfg(all(
        any(feature = "gossipsub", feature = "request-response"),
        not(feature = "byos")
    ))]
    last_scoring_decay_time: HashMap<PeerId, SystemTime>,
}

impl ScoreManager {
    /// Creates a new [`ScoreManager`] with empty score maps.
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "gossipsub")]
            gossipsub_app_score: HashMap::new(),

            #[cfg(feature = "request-response")]
            req_resp_app_score: HashMap::new(),

            #[cfg(any(feature = "request-response", feature = "gossipsub"))]
            last_scoring_decay_time: HashMap::new(),
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
        self.gossipsub_app_score.insert(*peer_id, new_score);
    }

    /// Updates the request-response application score for a specific peer.
    #[cfg(feature = "request-response")]
    pub fn update_req_resp_app_score(&mut self, peer_id: &PeerId, new_score: f64) {
        self.req_resp_app_score.insert(*peer_id, new_score);
    }

    /// Updates the last scoring decay time for a given [`PeerId`].
    ///
    /// Returns `Some(SystemTime)` containing the previous decay time if it existed,
    /// or `None` if this is the first time updating the peer.
    pub fn update_last_scoring_decay_time(
        &mut self,
        peer_id: PeerId,
        time: SystemTime,
    ) -> Option<SystemTime> {
        self.last_scoring_decay_time.insert(peer_id, time)
    }

    /// Removes all scores and tracking data for a specific peer.
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        #[cfg(feature = "gossipsub")]
        self.gossipsub_app_score.remove(peer_id);

        #[cfg(feature = "request-response")]
        self.req_resp_app_score.remove(peer_id);

        #[cfg(any(feature = "request-response", feature = "gossipsub"))]
        self.last_scoring_decay_time.remove(peer_id);
    }
}

/// All scores for a peer.
#[derive(Debug, Clone, Default)]
pub struct PeerScore {
    /// Peer's app scores.
    pub app_score: AppPeerScore,
    /// Gossipsub internal score.
    #[cfg(feature = "gossipsub")]
    pub gossipsub_internal_score: f64,
}

/// Peer's app scores.
#[derive(Debug, Clone, Default)]
pub struct AppPeerScore {
    /// Gossipsub application score.
    #[cfg(feature = "gossipsub")]
    pub gossipsub_app_score: f64,

    /// Request-response application score.
    #[cfg(feature = "request-response")]
    pub req_resp_app_score: f64,
}
