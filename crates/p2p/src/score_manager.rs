use std::collections::HashMap;

use libp2p::identity::PublicKey;

/// Default application score for request-response protocol.
pub const DEFAULT_REQ_RESP_APP_SCORE: f64 = 0.0;

/// Default application score for gossipsub protocol.
pub const DEFAULT_GOSSIP_APP_SCORE: f64 = 0.0;

/// Manages peer scoring for different protocols in the P2P network.
/// Tracks application-level scores for peers across gossipsub and request-response protocols.
#[derive(Debug, Clone, Default)]
pub struct ScoreManager {
    /// [`HashMap`] of `gossipsub` app score for each peer.
    gossipsub_app_score: HashMap<PublicKey, f64>,

    /// [`HashMap`] of `req-resp` score for each peer.
    req_resp_app_score: HashMap<PublicKey, f64>,
}

impl ScoreManager {
    /// Creates a new `ScoreManager` with empty score maps.
    pub fn new() -> Self {
        Self {
            gossipsub_app_score: HashMap::new(),
            req_resp_app_score: HashMap::new(),
        }
    }

    /// Retrieves the gossipsub application score for a specific peer.
    /// Returns `None` if no score exists for the peer.
    pub fn get_gossipsub_app_score(&self, app_public_key: &PublicKey) -> Option<f64> {
        self.gossipsub_app_score.get(app_public_key).cloned()
    }

    /// Retrieves the request-response application score for a specific peer.
    /// Returns `None` if no score exists for the peer.
    pub fn get_req_resp_score(&self, app_public_key: &PublicKey) -> Option<f64> {
        self.req_resp_app_score.get(app_public_key).cloned()
    }

    /// Updates the gossipsub application score for a specific peer.
    pub fn update_gossipsub_app_score(&mut self, app_public_key: &PublicKey, new_score: f64) {
        self.gossipsub_app_score
            .insert(app_public_key.clone(), new_score);
    }

    /// Updates the request-response application score for a specific peer.
    pub fn update_req_resp_app_score(&mut self, app_public_key: &PublicKey, new_score: f64) {
        self.req_resp_app_score
            .insert(app_public_key.clone(), new_score);
    }
}
