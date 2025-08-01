use std::collections::HashMap;

use libp2p::identity::PublicKey;

pub const DEFAULT_REQ_RESP_APP_SCORE: f64 = 0.0;
pub const DEFAULT_GOSSIP_APP_SCORE: f64 = 0.0;

#[derive(Debug, Clone, Default)]
pub struct ScoreManager {
    // Store gossipsub app score for each peer
    gossipsub_app_score: HashMap<PublicKey, f64>,

    // Store req-resp score for each peer
    req_resp_app_score: HashMap<PublicKey, f64>,
}

impl ScoreManager {
    pub fn new() -> Self {
        Self {
            gossipsub_app_score: HashMap::new(),
            req_resp_app_score: HashMap::new(),
        }
    }

    pub fn get_gossipsub_app_score(&self, app_public_key: &PublicKey) -> Option<f64> {
        self.gossipsub_app_score.get(app_public_key).cloned()
    }

    pub fn get_req_resp_score(&self, app_public_key: &PublicKey) -> Option<f64> {
        self.req_resp_app_score.get(app_public_key).cloned()
    }

    pub fn update_gossipsub_app_score(&mut self, app_public_key: &PublicKey, new_score: f64) {
        self.gossipsub_app_score
            .insert(app_public_key.clone(), new_score);
    }

    pub fn update_req_resp_app_score(&mut self, app_public_key: &PublicKey, new_score: f64) {
        self.req_resp_app_score
            .insert(app_public_key.clone(), new_score);
    }
}
