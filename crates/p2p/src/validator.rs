//! Validator for the P2P network.

#![cfg(not(feature = "byos"))]
use std::{
    collections::HashMap,
    fmt::Debug,
    time::{Duration, SystemTime},
};

use libp2p::identity::PeerId;

use crate::{commands::UnpenaltyType, score_manager::PeerScore};

/// Default ban period for peer misbehavior. Hardcoded to 30 days.
pub const DEFAULT_BAN_PERIOD: Duration = Duration::from_secs(60 * 60 * 24 * 30);

/// Default decay rate per second.
pub const DEFAULT_DECAY_RATE_PER_SEC: f64 = 0.9;

/// Default cost per message for rate limiting.
pub const DEFAULT_MESSAGE_COST: f64 = 1.0;

/// Default score threshold below which a peer is muted.
pub const DEFAULT_MUTE_THRESHOLD: f64 = -100.0;

/// Default duration to mute a peer when they cross the threshold.
pub const DEFAULT_MUTE_DURATION: Duration = Duration::from_secs(60);

/// Message types.
#[derive(Debug)]
pub enum Message {
    /// Gossipsub message.
    #[cfg(feature = "gossipsub")]
    Gossipsub(Vec<u8>),
    /// Request message.
    #[cfg(feature = "request-response")]
    Request(Vec<u8>),
    /// Response message.
    #[cfg(feature = "request-response")]
    Response(Vec<u8>),
}

/// Penalty types for peer misbehavior.
#[derive(Debug)]
pub enum PenaltyType {
    /// Mute gossipsub messages for duration.
    #[cfg(feature = "gossipsub")]
    MuteGossip(Duration),
    /// Mute request/response messages for duration.
    #[cfg(feature = "request-response")]
    MuteReqresp(Duration),
    /// Mute both protocols for duration.
    #[cfg(all(feature = "gossipsub", feature = "request-response"))]
    MuteBoth(Duration),
    /// Ban peer (None = permanent, Some = temporary).
    Ban(Option<Duration>),
}

/// Action type for peer moderation operations.
/// NOTE: BYOS uses an allowlist, making scoring system useless.
#[cfg(all(
    any(feature = "gossipsub", feature = "request-response"),
    not(feature = "byos")
))]
#[derive(Debug)]
pub enum Action {
    /// Apply a penalty to a peer.
    ApplyPenalty(PenaltyType),
    /// Remove a penalty from a peer.
    RemovePenalty(UnpenaltyType),
}

/// Penalty information for a peer.
#[derive(Debug)]
pub struct PenaltyInfo {
    /// Timestamp until which the peer is muted for gossipsub.
    #[cfg(feature = "gossipsub")]
    mute_gossip_until: Option<SystemTime>,
    /// Timestamp until which the peer is muted for request/response.
    #[cfg(feature = "request-response")]
    mute_req_resp_until: Option<SystemTime>,
    /// Timestamp until which the peer is banned.
    ban_until: Option<SystemTime>,
}

impl PenaltyInfo {
    /// Creates a new [`PenaltyInfo`].
    pub const fn new(
        #[cfg(feature = "gossipsub")] mute_gossip_until: Option<SystemTime>,
        #[cfg(feature = "request-response")] mute_req_resp_until: Option<SystemTime>,
        ban_until: Option<SystemTime>,
    ) -> Self {
        Self {
            #[cfg(feature = "gossipsub")]
            mute_gossip_until,
            #[cfg(feature = "request-response")]
            mute_req_resp_until,
            ban_until,
        }
    }
}

/// Storage for peer penalties.
#[derive(Debug, Default)]
pub struct PenaltyPeerStorage {
    penalties: HashMap<PeerId, PenaltyInfo>,
}

/// Validator trait. Used to validate messages and return penalties.
pub trait Validator: Debug + Send + Sync + 'static {
    /// Validates data using the generic validator and returns a new score.
    fn validate_msg(&self, msg: &Message, old_app_score: f64) -> f64;

    /// Returns the logic that is used to analyze and process message `msg`.
    fn get_penalty(&self, msg: &Message, peer_score: &PeerScore) -> Option<PenaltyType>;

    /// Applies score decay based on time since the last decay.
    fn apply_decay(&self, score: &f64, time_since_last_decay: &Duration) -> f64;
}

/// Default validator.
#[derive(Debug, Default, Clone)]
pub struct DefaultP2PValidator;

impl Validator for DefaultP2PValidator {
    #[allow(unused_variables)]
    fn validate_msg(&self, msg: &Message, old_app_score: f64) -> f64 {
        old_app_score - DEFAULT_MESSAGE_COST
    }

    #[allow(unused_variables)]
    fn get_penalty(&self, msg: &Message, peer_score: &PeerScore) -> Option<PenaltyType> {
        match msg {
            #[cfg(feature = "gossipsub")]
            Message::Gossipsub(_) => {
                if peer_score.app_score.gossipsub_app_score < DEFAULT_MUTE_THRESHOLD {
                    return Some(PenaltyType::MuteGossip(DEFAULT_MUTE_DURATION));
                }
            }
            #[cfg(feature = "request-response")]
            Message::Request(_) | Message::Response(_) => {
                if peer_score.app_score.req_resp_app_score < DEFAULT_MUTE_THRESHOLD {
                    return Some(PenaltyType::MuteReqresp(DEFAULT_MUTE_DURATION));
                }
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }
        None
    }

    fn apply_decay(&self, score: &f64, time_since_last_decay: &Duration) -> f64 {
        let delta_seconds = time_since_last_decay.as_secs_f64();
        let decay_increment = DEFAULT_DECAY_RATE_PER_SEC * delta_seconds;

        (score + decay_increment).min(0.0)
    }
}

impl PenaltyPeerStorage {
    /// Creates a new [`PenaltyPeerStorage`].
    pub fn new() -> Self {
        Self {
            penalties: HashMap::new(),
        }
    }

    /// Checks if the peer is muted for gossip.
    #[cfg(feature = "gossipsub")]
    pub fn is_gossip_muted(&self, peer_id: &PeerId) -> bool {
        self.penalties
            .get(peer_id)
            .and_then(|penalty| penalty.mute_gossip_until)
            .is_some_and(|timestamp| timestamp > SystemTime::now())
    }

    /// Checks if the peer is muted for request/response.
    #[cfg(feature = "request-response")]
    pub fn is_req_resp_muted(&self, peer_id: &PeerId) -> bool {
        self.penalties
            .get(peer_id)
            .and_then(|penalty| penalty.mute_req_resp_until)
            .is_some_and(|timestamp| timestamp > SystemTime::now())
    }

    /// Checks if the peer is banned.
    pub fn is_banned(&self, peer_id: &PeerId) -> bool {
        self.penalties
            .get(peer_id)
            .and_then(|penalty| penalty.ban_until)
            .is_some_and(|timestamp| timestamp > SystemTime::now())
    }

    /// Mutes the peer for gossip for the given duration.
    #[cfg(feature = "gossipsub")]
    pub fn mute_peer_gossip(
        &mut self,
        peer_id: &PeerId,
        until: SystemTime,
    ) -> Result<(), &'static str> {
        let penalty = self.penalties.entry(*peer_id).or_insert_with(|| {
            PenaltyInfo::new(
                None,
                None,
                #[cfg(feature = "request-response")]
                None,
            )
        });

        if let Some(mute_until) = penalty.mute_gossip_until
            && mute_until > SystemTime::now()
        {
            penalty.mute_gossip_until = Some(mute_until.max(until));
            return Ok(());
        }

        penalty.mute_gossip_until = Some(until);
        Ok(())
    }

    /// Mutes the peer for request/response for the given duration.
    #[cfg(feature = "request-response")]
    pub fn mute_peer_req_resp(
        &mut self,
        peer_id: &PeerId,
        until: SystemTime,
    ) -> Result<(), &'static str> {
        let penalty = self.penalties.entry(*peer_id).or_insert_with(|| {
            PenaltyInfo::new(
                #[cfg(feature = "gossipsub")]
                None,
                #[cfg(feature = "request-response")]
                None,
                None,
            )
        });

        if let Some(mute_until) = penalty.mute_req_resp_until
            && mute_until > SystemTime::now()
        {
            penalty.mute_req_resp_until = Some(mute_until.max(until));
            return Ok(());
        }

        penalty.mute_req_resp_until = Some(until);
        Ok(())
    }

    /// Bans the peer for the given duration.
    pub fn ban_peer(&mut self, peer_id: &PeerId, until: SystemTime) -> Result<(), &'static str> {
        let penalty = self.penalties.entry(*peer_id).or_insert_with(|| {
            PenaltyInfo::new(
                #[cfg(feature = "gossipsub")]
                None,
                #[cfg(feature = "request-response")]
                None,
                None,
            )
        });

        if let Some(ban_until) = penalty.ban_until
            && ban_until > SystemTime::now()
        {
            penalty.ban_until = Some(ban_until.max(until));
            return Ok(());
        }

        penalty.ban_until = Some(until);
        Ok(())
    }

    /// Removes an active ban from the peer, if any.
    pub fn unban_peer(&mut self, peer_id: &PeerId) {
        if let Some(penalty) = self.penalties.get_mut(peer_id) {
            penalty.ban_until = None;
        }
    }

    /// Removes an active gossipsub mute from the peer, if any.
    #[cfg(feature = "gossipsub")]
    pub fn unmute_peer_gossip(&mut self, peer_id: &PeerId) {
        if let Some(penalty) = self.penalties.get_mut(peer_id) {
            penalty.mute_gossip_until = None;
        }
    }

    /// Removes an active request/response mute from the peer, if any.
    #[cfg(feature = "request-response")]
    pub fn unmute_peer_req_resp(&mut self, peer_id: &PeerId) {
        if let Some(penalty) = self.penalties.get_mut(peer_id) {
            penalty.mute_req_resp_until = None;
        }
    }

    /// Checks if the peer has any active penalties.
    pub fn has_active_penalties(&self, peer_id: &PeerId) -> bool {
        if let Some(penalty) = self.penalties.get(peer_id) {
            if penalty.ban_until.is_some_and(|t| t > SystemTime::now()) {
                return true;
            }
            #[cfg(feature = "gossipsub")]
            if penalty
                .mute_gossip_until
                .is_some_and(|t| t > SystemTime::now())
            {
                return true;
            }
            #[cfg(feature = "request-response")]
            if penalty
                .mute_req_resp_until
                .is_some_and(|t| t > SystemTime::now())
            {
                return true;
            }
        }
        false
    }

    /// Removes all penalty information for a peer.
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        self.penalties.remove(peer_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_validator_scoring() {
        let validator = DefaultP2PValidator;

        // Determine a message type available in current config
        #[cfg(feature = "gossipsub")]
        let msg = Message::Gossipsub(vec![]);
        #[cfg(all(not(feature = "gossipsub"), feature = "request-response"))]
        let msg = Message::Request(vec![]);

        #[cfg(any(feature = "gossipsub", feature = "request-response"))]
        {
            // Test validate_msg
            let initial_score = 0.0;
            let new_score = validator.validate_msg(&msg, initial_score);
            assert_eq!(new_score, initial_score - DEFAULT_MESSAGE_COST);

            // Test decay
            let decayed = validator.apply_decay(&new_score, &Duration::from_secs(1));
            assert!((decayed - (new_score + DEFAULT_DECAY_RATE_PER_SEC)).abs() < 1e-6);

            // Test penalty
            let mut peer_score = PeerScore::default();

            #[cfg(feature = "gossipsub")]
            if matches!(msg, Message::Gossipsub(_)) {
                peer_score.app_score.gossipsub_app_score = DEFAULT_MUTE_THRESHOLD - 1.0;
                let penalty = validator.get_penalty(&msg, &peer_score);
                if let Some(PenaltyType::MuteGossip(duration)) = penalty {
                    assert_eq!(duration, DEFAULT_MUTE_DURATION);
                } else {
                    panic!("Expected MuteGossip penalty");
                }
            }

            #[cfg(feature = "request-response")]
            if matches!(msg, Message::Request(_)) {
                peer_score.app_score.req_resp_app_score = DEFAULT_MUTE_THRESHOLD - 1.0;
                let penalty = validator.get_penalty(&msg, &peer_score);
                if let Some(PenaltyType::MuteReqresp(duration)) = penalty {
                    assert_eq!(duration, DEFAULT_MUTE_DURATION);
                } else {
                    panic!("Expected MuteReqresp penalty");
                }
            }
        }
    }

    #[test]
    fn test_penalty_extension() {
        let mut storage = PenaltyPeerStorage::new();
        let peer_id = PeerId::random();
        let now = SystemTime::now();
        let one_min = Duration::from_secs(60);
        let two_mins = Duration::from_secs(120);

        // Test Ban extension (always available)
        {
            storage.ban_peer(&peer_id, now + one_min).unwrap();
            assert!(storage.is_banned(&peer_id));

            let result = storage.ban_peer(&peer_id, now + two_mins);
            assert!(result.is_ok());
            assert!(storage.is_banned(&peer_id));
        }

        // Test Gossipsub mute extension
        #[cfg(feature = "gossipsub")]
        {
            storage.mute_peer_gossip(&peer_id, now + one_min).unwrap();
            assert!(storage.is_gossip_muted(&peer_id));

            let result = storage.mute_peer_gossip(&peer_id, now + two_mins);
            assert!(result.is_ok());
            assert!(storage.is_gossip_muted(&peer_id));
        }

        // Test Request-Response mute extension
        #[cfg(feature = "request-response")]
        {
            storage.mute_peer_req_resp(&peer_id, now + one_min).unwrap();
            assert!(storage.is_req_resp_muted(&peer_id));

            let result = storage.mute_peer_req_resp(&peer_id, now + two_mins);
            assert!(result.is_ok());
            assert!(storage.is_req_resp_muted(&peer_id));
        }
    }
}
