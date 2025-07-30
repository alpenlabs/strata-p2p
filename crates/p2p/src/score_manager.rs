use std::collections::HashMap;

use libp2p::PeerId;

pub const DEFAULT_DECAY_FACTOR: f64 = 0.9;
pub const DEFAULT_REQ_RESP_APP_SCORE: f64 = 0.0;
pub const DEFAULT_GOSSIP_APP_SCORE: f64 = 0.0;

#[derive(Debug, Clone)]
pub struct ScoreManager {
    // Store gossipsub app score for each peer
    gossipsub_app_score: HashMap<PeerId, f64>,

    // Store req-resp score for each peer
    req_resp_app_score: HashMap<PeerId, f64>,

    // decay factor for scores
    decay_factor: f64,
}

impl ScoreManager {
    pub fn new(decay_factor: f64) -> Self {
        Self {
            gossipsub_app_score: HashMap::new(),
            req_resp_app_score: HashMap::new(),
            decay_factor,
        }
    }

    pub fn get_gossipsub_app_score(&self, peer_id: &PeerId) -> Option<f64> {
        self.gossipsub_app_score.get(peer_id).cloned()
    }

    pub fn get_req_resp_score(&self, peer_id: &PeerId) -> Option<f64> {
        self.req_resp_app_score.get(peer_id).cloned()
    }

    pub fn update_gossipsub_app_score(&mut self, peer_id: &PeerId, new_score: f64) {
        self.gossipsub_app_score.insert(*peer_id, new_score);
    }

    pub fn update_req_resp_app_score(&mut self, peer_id: &PeerId, new_score: f64) {
        self.req_resp_app_score.insert(*peer_id, new_score);
    }

    pub fn apply_decay(&mut self) {
        for (_peer_id, score) in self.gossipsub_app_score.iter_mut() {
            *score *= self.decay_factor;
        }
        for (_peer_id, score) in self.req_resp_app_score.iter_mut() {
            *score *= self.decay_factor;
        }
    }
}
