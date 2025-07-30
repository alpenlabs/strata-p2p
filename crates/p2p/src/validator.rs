use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use libp2p::PeerId;

pub const DEFAULT_BAN_PERIOD: Duration = Duration::from_secs(60 * 60 * 24 * 30);

#[derive(Debug)]
pub enum Message {
    Gossipsub(Vec<u8>),
    Request(Vec<u8>),
    Response(Vec<u8>),
}

#[derive(Debug)]
pub enum PenaltyType {
    Ignore,
    MuteGossip(Duration),  // Mute this peer for some time in Gossipsub
    MuteReqresp(Duration), // Mute this peer for some time in RequestResponse
    MuteBoth(Duration),    // Mute this peer for both protocols
    Ban(Option<Duration>), // Ban, optional duration (None = forever)
}

#[derive(Debug)]
pub struct PenaltyInfo {
    mute_gossip_until: Option<SystemTime>, // None if not muted for Gossipsub
    mute_req_resp_until: Option<SystemTime>, // None if not muted for RequestResponse
    ban_until: Option<SystemTime>,         // None if not banned
}

impl PenaltyInfo {
    fn new(
        mute_gossip_until: Option<SystemTime>,
        mute_req_resp_until: Option<SystemTime>,
        ban_until: Option<SystemTime>,
    ) -> Self {
        Self {
            mute_gossip_until,
            mute_req_resp_until,
            ban_until,
        }
    }
}

#[derive(Debug)]
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

#[derive(Debug, Default)]
pub struct DefaultP2PValidator;

impl Validator for DefaultP2PValidator {
    fn validate_msg(_msg: &Message, _old_app_score: f64) -> f64 {
        0.0
    }

    fn get_penalty(
        _msg: &Message,
        _gossip_internal_score: f64,
        _gossip_app_score: f64,
        _reqresp_app_score: f64,
    ) -> Option<PenaltyType> {
        None
    }
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
            .map_or(false, |timestamp| timestamp > SystemTime::now())
    }

    pub fn is_req_resp_muted(&self, peer_id: &PeerId) -> bool {
        self.penalties
            .get(peer_id)
            .and_then(|penalty| penalty.mute_req_resp_until)
            .map_or(false, |timestamp| timestamp > SystemTime::now())
    }

    pub fn is_banned(&self, peer_id: &PeerId) -> bool {
        self.penalties
            .get(peer_id)
            .and_then(|penalty| penalty.ban_until)
            .map_or(false, |timestamp| timestamp > SystemTime::now())
    }

    pub fn mute_peer_gossip(
        &mut self,
        peer_id: &PeerId,
        until: SystemTime,
    ) -> Result<(), &'static str> {
        let penalty = self
            .penalties
            .entry(peer_id.clone())
            .or_insert_with(|| PenaltyInfo::new(None, None, None));

        if let Some(mute_until) = penalty.mute_gossip_until {
            if mute_until > SystemTime::now() {
                return Err("Peer is already muted for gossip");
            }
        }

        penalty.mute_gossip_until = Some(until);
        Ok(())
    }

    pub fn mute_peer_req_resp(
        &mut self,
        peer_id: &PeerId,
        until: SystemTime,
    ) -> Result<(), &'static str> {
        let penalty = self
            .penalties
            .entry(peer_id.clone())
            .or_insert_with(|| PenaltyInfo::new(None, None, None));

        if let Some(mute_until) = penalty.mute_req_resp_until {
            if mute_until > SystemTime::now() {
                return Err("Peer is already muted for request/response");
            }
        }

        penalty.mute_req_resp_until = Some(until);
        Ok(())
    }

    pub fn ban_peer(&mut self, peer_id: &PeerId, until: SystemTime) -> Result<(), &'static str> {
        let penalty = self
            .penalties
            .entry(peer_id.clone())
            .or_insert_with(|| PenaltyInfo::new(None, None, None));

        if let Some(ban_until) = penalty.ban_until {
            if ban_until > SystemTime::now() {
                return Err("Peer is already banned");
            }
        }

        penalty.ban_until = Some(until);
        Ok(())
    }
}
