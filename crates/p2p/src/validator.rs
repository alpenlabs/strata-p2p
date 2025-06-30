use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Utc};
use libp2p::PeerId;

enum Message {
    Gossipsub(Vec<u8>),
    Request(Vec<u8>),
    Response(Vec<u8>),
}

enum PenaltyType {
    Ignore,
    MuteGossip(Duration),  // Mute this peer for some time in Gossipsub
    MuteReqresp(Duration), // Mute this peer for some time in RequestResponse
    MuteBoth(Duration),    // Mute this peer for both protocols
    Ban(Option<Duration>), // Ban, optional duration (None = forever)
}

struct PenaltyInfo {
    mute_gossip_until: Option<DateTime<Utc>>, // None if not muted for Gossipsub
    mute_req_resp_until: Option<DateTime<Utc>>, // None if not muted for RequestResponse
    ban_until: Option<DateTime<Utc>>,         // None if not banned
}

impl PenaltyInfo {
    fn new(
        mute_gossip_until: Option<DateTime<Utc>>,
        mute_req_resp_until: Option<DateTime<Utc>>,
        ban_until: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            mute_gossip_until,
            mute_req_resp_until,
            ban_until,
        }
    }
}

pub struct PenaltyPeerStorage {
    penalties: HashMap<PeerId, PenaltyInfo>,
}

pub trait Validator {
    /// Validates data using the generic validator.
    fn validate_msg(msg: &Message, old_app_score: f64) -> f64 /* new_app_score */;

    /// Logic where we analyze the message and decide what to do with it.
    fn get_penalty(
        msg: &Message,
        gossip_internal_score: f64,
        gossip_app_score: f64,
        reqresp_app_score: f64,
    ) -> Option<PenaltyType>;
}

impl PenaltyPeerStorage {
    pub fn new() -> Self {
        Self {
            penalties: HashMap::new(),
        }
    }
    pub fn is_gossip_muted(&self, peer_id: &PeerId) -> bool {
        self.penalties
            .get(peer_id)
            .and_then(|penalty| penalty.mute_gossip_until)
            .map_or(false, |timestamp| timestamp > Utc::now())
    }

    pub fn is_req_resp_muted(&self, peer_id: &PeerId) -> bool {
        self.penalties
            .get(peer_id)
            .and_then(|penalty| penalty.mute_req_resp_until)
            .map_or(false, |timestamp| timestamp > Utc::now())
    }

    pub fn is_banned(&self, peer_id: &PeerId) -> bool {
        self.penalties
            .get(peer_id)
            .and_then(|penalty| penalty.ban_until)
            .map_or(false, |timestamp| timestamp > Utc::now())
    }

    pub fn mute_peer_gossip(
        &mut self,
        peer_id: PeerId,
        until: DateTime<Utc>,
    ) -> Result<(), &'static str> {
        let penalty = self
            .penalties
            .entry(peer_id.clone())
            .or_insert_with(|| PenaltyInfo::new(None, None, None));

        if let Some(mute_until) = penalty.mute_gossip_until {
            if mute_until > Utc::now() {
                return Err("Peer is already muted for Gossipsub");
            }
        }

        penalty.mute_gossip_until = Some(until);
        Ok(())
    }

    pub fn mute_peer_req_resp(
        &mut self,
        peer_id: PeerId,
        until: DateTime<Utc>,
    ) -> Result<(), &'static str> {
        let penalty = self
            .penalties
            .entry(peer_id.clone())
            .or_insert_with(|| PenaltyInfo::new(None, None, None));

        if let Some(mute_until) = penalty.mute_req_resp_until {
            if mute_until > Utc::now() {
                return Err("Peer is already muted for RequestResponse");
            }
        }

        penalty.mute_req_resp_until = Some(until);
        Ok(())
    }

    pub fn ban_peer(
        &mut self,
        peer_id: PeerId,
        until: Option<DateTime<Utc>>,
    ) -> Result<(), &'static str> {
        let penalty = self
            .penalties
            .entry(peer_id.clone())
            .or_insert_with(|| PenaltyInfo::new(None, None, None));

        if let Some(ban_until) = penalty.ban_until {
            if ban_until > Utc::now() {
                return Err("Peer is already banned");
            }
        }

        penalty.ban_until = until;
        Ok(())
    }
}
