//! Swarm implementation for P2P.

#[cfg(any(feature = "gossipsub", feature = "request-response"))]
use std::time::SystemTime;
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};
#[cfg(not(feature = "byos"))]
use std::num::NonZeroU8;

use behavior::{Behaviour, BehaviourEvent};
#[cfg(feature = "byos")]
use cynosure::site_c::queue::Queue;
use errors::{P2PResult, ProtocolError};
use futures::StreamExt as _;
#[cfg(not(all(feature = "gossipsub", feature = "request-response")))]
use futures::future::pending;
use handle::CommandHandle;
use libp2p::{
    Multiaddr, PeerId, Swarm, SwarmBuilder, Transport,
    core::{ConnectedPoint, muxing::StreamMuxerBox, transport::MemoryTransport},
    identity::{Keypair, PublicKey},
    noise,
    swarm::{NetworkBehaviour, SwarmEvent, dial_opts::DialOpts},
    yamux,
};
use tokio::{sync::mpsc, time::timeout};
use tokio_util::sync::CancellationToken;
#[cfg(any(feature = "gossipsub", feature = "request-response"))]
use tracing::instrument;
use tracing::{debug, error, info, trace, warn};
#[cfg(feature = "gossipsub")]
use {
    crate::{
        commands::GossipCommand, events::GossipEvent, score_manager::DEFAULT_GOSSIP_APP_SCORE,
        swarm::message::GossipMessage,
    },
    handle::GossipHandle,
    libp2p::gossipsub::{
        Event as GossipsubEvent, Message, MessageAcceptance, MessageId, PeerScoreParams,
        PeerScoreThresholds, PublishError, Sha256Topic,
    },
    std::sync::LazyLock,
    tokio::sync::broadcast,
};
#[cfg(feature = "request-response")]
use {
    crate::{
        commands::RequestResponseCommand,
        events::ReqRespEvent,
        score_manager::DEFAULT_REQ_RESP_APP_SCORE,
        swarm::message::{RequestMessage, ResponseMessage},
    },
    errors::SwarmError,
    handle::ReqRespHandle,
    libp2p::request_response::{self, Event as RequestResponseEvent},
    tokio::sync::oneshot,
};

use crate::{
    commands::{Command, QueryP2PStateCommand},
    score_manager::ScoreManager,
    signer::ApplicationSigner,
    swarm::{dial_manager::DialManager, setup::events::SetupBehaviourEvent},
    validator::{DefaultP2PValidator, PenaltyPeerStorage, Validator},
};
#[cfg(any(feature = "gossipsub", feature = "request-response"))]
use crate::{
    score_manager::PeerScore,
    swarm::message::SignedMessage,
    validator::{DEFAULT_BAN_PERIOD, Message as MessageType, PenaltyType},
};

pub mod behavior;
mod codec_raw;
pub mod dial_manager;
pub mod errors;
pub mod handle;
mod message;
pub mod setup;

use libp2p::tcp;

/// Global topic name for gossipsub messages.
// TODO: make this configurable later
#[cfg(feature = "gossipsub")]
static TOPIC: LazyLock<Sha256Topic> = LazyLock::new(|| Sha256Topic::new("bitvm2"));

/// Global MAX_TRANSMIT_SIZE for gossipsub messages.
#[cfg(feature = "gossipsub")]
const MAX_TRANSMIT_SIZE: usize = 512 * 1024;

/// Global name of the protocol
// TODO: make this configurable later
const PROTOCOL_NAME: &str = "/strata-bitvm2";

/// Global default retry count for connection attempts.
pub const DEFAULT_MAX_RETRIES: usize = 3;

/// Global timeout for dialing a peer.
pub const DEFAULT_DIAL_TIMEOUT: Duration = Duration::from_millis(250);

/// Global default timeout for general operations.
pub const DEFAULT_GENERAL_TIMEOUT: Duration = Duration::from_millis(250);

/// Global default interval for connection checks.
pub const DEFAULT_CONNECTION_CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// Global default timeout for channel operations (e.g., sending/receiving on channels).
pub const DEFAULT_CHANNEL_TIMEOUT: Duration = Duration::from_secs(5);

/// Default buffer size for gossip event broadcast channels.
#[cfg(feature = "gossipsub")]
pub const DEFAULT_GOSSIP_EVENT_BUFFER_SIZE: usize = 256;

/// Default buffer size for command channels.
pub const DEFAULT_COMMAND_BUFFER_SIZE: usize = 64;

/// Default buffer size for request-response event channels.
#[cfg(feature = "request-response")]
pub const DEFAULT_REQ_RESP_EVENT_BUFFER_SIZE: usize = 64;

/// Default buffer size for gossip command channels.
#[cfg(feature = "gossipsub")]
pub const DEFAULT_GOSSIP_COMMAND_BUFFER_SIZE: usize = 64;

/// Configuration options for [`P2P`].
#[derive(Debug, Clone)]
pub struct P2PConfig {
    /// Long-term application public key.
    pub app_public_key: PublicKey,

    /// Ephemeral transport keypair.
    pub transport_keypair: Keypair,

    /// Idle connection timeout.
    pub idle_connection_timeout: Duration,

    /// Max retry count for connections.
    ///
    /// The default is [`DEFAULT_MAX_RETRIES`].
    pub max_retries: Option<usize>,

    /// Dial timeout.
    ///
    /// The default is [`DEFAULT_DIAL_TIMEOUT`].
    pub dial_timeout: Option<Duration>,

    /// General timeout for operations.
    ///
    /// The default is [`DEFAULT_GENERAL_TIMEOUT`].
    pub general_timeout: Option<Duration>,

    /// Connection check interval.
    ///
    /// The default is [`DEFAULT_CONNECTION_CHECK_INTERVAL`].
    pub connection_check_interval: Option<Duration>,

    /// The node's listening addresses.
    pub listening_addrs: Vec<Multiaddr>,

    /// Initial list of nodes to connect to at startup.
    pub connect_to: Vec<Multiaddr>,

    /// Timeout for channel operations (e.g., sending/receiving on channels).
    #[cfg(feature = "request-response")]
    pub channel_timeout: Option<Duration>,

    /// Gossipsub peer scoring parameters.
    ///
    /// If [`None`], the default parameters will be used.
    /// Use this to fine-tune how peers are scored for message delivery, invalid messages, etc.
    /// See [`PeerScoreParams`] for all available options.
    #[cfg(feature = "gossipsub")]
    pub gossipsub_score_params: Option<PeerScoreParams>,

    /// Gossipsub peer score thresholds.
    ///
    /// If [`None`], the default thresholds will be used.
    /// These thresholds determine when peers are muted, graylisted, or banned based on their
    /// score. See [`PeerScoreThresholds`] for details.
    #[cfg(feature = "gossipsub")]
    pub gossipsub_score_thresholds: Option<PeerScoreThresholds>,

    /// Buffer size for gossip event broadcast channels.
    ///
    /// If [`None`], the default buffer size will be used.
    /// The default is [`DEFAULT_GOSSIP_EVENT_BUFFER_SIZE`].
    #[cfg(feature = "gossipsub")]
    pub gossip_event_buffer_size: Option<usize>,

    /// Buffer size for core command channels.
    ///
    /// If [`None`], the default buffer size will be used.
    /// The default is [`DEFAULT_COMMAND_BUFFER_SIZE`].
    pub command_buffer_size: Option<usize>,

    /// Buffer size for request-response event channels.
    ///
    /// If [`None`], the default buffer size will be used.
    /// The default is [`DEFAULT_REQ_RESP_EVENT_BUFFER_SIZE`].
    #[cfg(feature = "request-response")]
    pub req_resp_event_buffer_size: Option<usize>,

    /// Buffer size for gossip command channels.
    ///
    /// If [`None`], the default buffer size will be used.
    /// The default is [`DEFAULT_GOSSIP_COMMAND_BUFFER_SIZE`].
    #[cfg(feature = "gossipsub")]
    pub gossip_command_buffer_size: Option<usize>,
}

/// Implementation of P2P protocol data exchange.
#[expect(missing_debug_implementations)]
pub struct P2P {
    /// The swarm that handles the networking.
    swarm: Swarm<Behaviour>,

    /// Event channel for the gossip.
    #[cfg(feature = "gossipsub")]
    gossip_events: broadcast::Sender<GossipEvent>,

    /// Event channel for request/response.
    #[cfg(feature = "request-response")]
    req_resp_events: mpsc::Sender<ReqRespEvent>,

    /// Command channel for the swarm.
    commands: mpsc::Receiver<Command>,

    /// Gossip command channel for handles.
    #[cfg(feature = "gossipsub")]
    gossip_commands: mpsc::Receiver<GossipCommand>,

    /// Request-response command channel for handles.
    #[cfg(feature = "request-response")]
    request_response_commands: mpsc::Receiver<RequestResponseCommand>,

    /// ([`Clone`]able) Command channel for the swarm.
    ///
    /// # Implementation details
    ///
    /// This is needed because we can't create new handles from the receiver, as
    /// only sender is [`Clone`]able.
    commands_sender: mpsc::Sender<Command>,

    /// Gossip command sender for creating handles.
    #[cfg(feature = "gossipsub")]
    gossip_commands_sender: mpsc::Sender<GossipCommand>,

    /// Cancellation token for the swarm.
    cancellation_token: CancellationToken,

    /// Underlying configuration.
    config: P2PConfig,

    /// Allow list.
    allowlist: HashSet<PublicKey>,

    /// Application signer for signing setup messages.
    #[cfg_attr(
        not(any(feature = "gossipsub", feature = "request-response")),
        allow(dead_code)
    )]
    signer: Arc<dyn ApplicationSigner>,

    /// Manages dial sequences and address queues for multiaddress connections.
    dial_manager: DialManager,

    /// Score manager.
    #[cfg_attr(
        not(any(feature = "gossipsub", feature = "request-response")),
        allow(dead_code)
    )]
    score_manager: ScoreManager,

    /// Storage with penalty for peer's penalty.
    peer_penalty_storage: PenaltyPeerStorage,

    /// Manages message validation and penalty logic.
    #[cfg_attr(
        not(any(feature = "gossipsub", feature = "request-response")),
        allow(dead_code)
    )]
    validator: Box<dyn Validator>,
}

/// Type alias that changes based on feature flags
#[cfg(feature = "request-response")]
pub type P2PFromConfig = (P2P, ReqRespHandle);

/// Type alias that changes based on feature flags
#[cfg(not(feature = "request-response"))]
pub type P2PFromConfig = P2P;

impl P2P {
    /// Creates a new P2P instance from the given configuration.
    pub fn from_config(
        cfg: P2PConfig,
        cancel: CancellationToken,
        mut swarm: Swarm<Behaviour>,
        allowlist: Vec<PublicKey>,
        #[cfg(feature = "gossipsub")] channel_size: Option<usize>,
        signer: Arc<dyn ApplicationSigner>,
        validator: Option<Box<dyn Validator>>,
    ) -> P2PResult<P2PFromConfig> {
        for addr in &cfg.listening_addrs {
            swarm
                .listen_on(addr.clone())
                .map_err(ProtocolError::Listen)?;
        }

        // Core components setup
        let command_buffer_size = cfg
            .command_buffer_size
            .unwrap_or(DEFAULT_COMMAND_BUFFER_SIZE);
        let (cmds_tx, cmds_rx) = mpsc::channel(command_buffer_size);
        let score_manager = ScoreManager::new();
        let peer_penalty_storage = PenaltyPeerStorage::new();

        // Request-response setup (only when feature is enabled)
        #[cfg(feature = "request-response")]
        let (req_resp_event_tx, req_resp_event_rx, req_resp_cmds_tx, req_resp_cmds_rx) = {
            let req_resp_event_buffer_size = cfg
                .req_resp_event_buffer_size
                .unwrap_or(DEFAULT_REQ_RESP_EVENT_BUFFER_SIZE);
            let (req_resp_event_tx, req_resp_event_rx) =
                mpsc::channel::<ReqRespEvent>(req_resp_event_buffer_size);
            let (req_resp_cmds_tx, req_resp_cmds_rx) = mpsc::channel(command_buffer_size);
            (req_resp_event_tx, req_resp_event_rx, req_resp_cmds_tx, req_resp_cmds_rx)
        };

        // Gossipsub setup (only when feature is enabled)
        #[cfg(feature = "gossipsub")]
        let (gossip_events_tx, gossip_cmds_tx, gossip_cmds_rx) = {
            let gossip_event_buffer_size = cfg
                .gossip_event_buffer_size
                .unwrap_or(DEFAULT_GOSSIP_EVENT_BUFFER_SIZE);
            let channel_size = channel_size.unwrap_or(gossip_event_buffer_size);
            let (gossip_events_tx, _gossip_events_rx) = broadcast::channel(channel_size);
            let gossip_command_buffer_size = cfg
                .gossip_command_buffer_size
                .unwrap_or(DEFAULT_GOSSIP_COMMAND_BUFFER_SIZE);
            let (gossip_cmds_tx, gossip_cmds_rx) = mpsc::channel(gossip_command_buffer_size);
            (gossip_events_tx, gossip_cmds_tx, gossip_cmds_rx)
        };

        let validator = validator.unwrap_or_else(|| Box::new(DefaultP2PValidator::default()));

        let p2p = P2P {
            swarm,
            #[cfg(feature = "gossipsub")]
            gossip_events: gossip_events_tx,
            #[cfg(feature = "request-response")]
            req_resp_events: req_resp_event_tx,
            #[cfg(feature = "request-response")]
            request_response_commands: req_resp_cmds_rx,
            #[cfg(feature = "gossipsub")]
            gossip_commands: gossip_cmds_rx,
            #[cfg(feature = "gossipsub")]
            gossip_commands_sender: gossip_cmds_tx.clone(),
            commands: cmds_rx,
            commands_sender: cmds_tx.clone(),
            cancellation_token: cancel,
            allowlist: HashSet::from_iter(allowlist),
            config: cfg,
            signer,
            dial_manager: DialManager::new(),
            score_manager,
            peer_penalty_storage,
            validator,
        };

        #[cfg(feature = "request-response")]
        return Ok((p2p, ReqRespHandle::new(req_resp_event_rx, req_resp_cmds_tx)));

        #[cfg(not(feature = "request-response"))]
        return Ok(p2p);
    }

    /// Returns the [`PeerId`] of the local node.
    pub fn local_peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }

    /// Creates new handle for gossip.
    #[cfg(feature = "gossipsub")]
    pub fn new_gossip_handle(&self) -> GossipHandle {
        GossipHandle::new(
            self.gossip_events.subscribe(),
            self.gossip_commands_sender.clone(),
        )
    }

    /// Creates new handle for commands.
    pub fn new_command_handle(&self) -> CommandHandle {
        CommandHandle::new(self.commands_sender.clone())
    }

    /// Waits until all connections are established and all peers are subscribed to
    /// current one.
    pub async fn establish_connections(&mut self) {
        let max_retry_count = self.config.max_retries.unwrap_or(DEFAULT_MAX_RETRIES);
        let dial_timeout = self.config.dial_timeout.unwrap_or(DEFAULT_DIAL_TIMEOUT);
        let general_timeout = self
            .config
            .general_timeout
            .unwrap_or(DEFAULT_GENERAL_TIMEOUT);
        let connection_check_interval = self
            .config
            .connection_check_interval
            .unwrap_or(DEFAULT_CONNECTION_CHECK_INTERVAL);

        let mut is_not_connected = HashSet::new();

        for addr in &self.config.connect_to {
            let mut num_retries = 0;
            debug!(%addr, %num_retries, %max_retry_count, "attempting to dial peer");
            loop {
                match self.swarm.dial(addr.clone()) {
                    Ok(_) => {
                        info!(%addr, "dialed peer");
                        break;
                    }
                    Err(err) => {
                        warn!(%err, %addr, %num_retries, %max_retry_count, "failed to connect to peer, retrying...");
                    }
                }

                num_retries += 1;

                if num_retries > max_retry_count {
                    error!(%addr, %num_retries, %max_retry_count, "failed to connect to peer after max retries");
                    break;
                }

                // Add a small delay between retries to avoid overwhelming the network
                tokio::time::sleep(dial_timeout).await;
                debug!(%addr, %num_retries, %max_retry_count, "attempting to dial peer again");
            }

            debug!(%addr, %num_retries, %max_retry_count, "finished trying to dial peer");
            is_not_connected.insert(addr);
        }

        #[cfg(feature = "gossipsub")]
        {
            let mut num_retries = 0;
            loop {
                debug!(topic=%TOPIC.to_string(), %num_retries, %max_retry_count, "attempting to subscribe to topic");
                match timeout(general_timeout, async {
                    self.swarm.behaviour_mut().gossipsub.subscribe(&TOPIC)
                })
                .await
                {
                    Ok(Ok(_)) => {
                        info!(topic=%TOPIC.to_string(), %num_retries, %max_retry_count, "subscribed to topic successfully");
                        break;
                    }
                    Ok(Err(err)) => {
                        error!(topic=%TOPIC.to_string(), %err, %num_retries, %max_retry_count, "failed to subscribe to topic, retrying...");
                    }
                    Err(_) => {
                        error!(topic=%TOPIC.to_string(), %num_retries, %max_retry_count, "failed to subscribe to topic, retrying...");
                    }
                }

                num_retries += 1;

                if num_retries > max_retry_count {
                    error!(topic=%TOPIC.to_string(), %num_retries, %max_retry_count, "failed to subscribe to topic after max retries");
                    break;
                }

                // Add a small delay between retries
                tokio::time::sleep(connection_check_interval).await;
                debug!(topic=%TOPIC.to_string(), %num_retries, %max_retry_count, "attempting to subscribe to topic again");
            }
            debug!(topic=%TOPIC.to_string(), %num_retries, %max_retry_count, "finished trying to subscribe to topic");
        }

        #[cfg(feature = "gossipsub")]
        let mut subscriptions = 0;
        #[cfg(not(feature = "gossipsub"))]
        let subscriptions = 0;
        let start_time = Instant::now();
        let mut next_check = Instant::now();

        let connection_future = async {
            while let Some(event) = self.swarm.next().await {
                debug!("received event from swarm");
                trace!(?event, "received event from swarm");

                match event {
                    #[cfg(feature = "gossipsub")]
                    SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(
                        GossipsubEvent::Subscribed { peer_id, .. },
                    )) => {
                        subscriptions += 1;
                        info!(%peer_id, %subscriptions, total=self.allowlist.len(), "got subscription");
                    }
                    SwarmEvent::ConnectionEstablished {
                        peer_id, endpoint, ..
                    } => {
                        let ConnectedPoint::Dialer { address, .. } = endpoint else {
                            continue;
                        };
                        info!(%address, %peer_id, "established connection with peer");
                        is_not_connected.remove(&address);
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        warn!(?peer_id, %error, "outgoing connection error");
                    }
                    SwarmEvent::IncomingConnectionError {
                        connection_id,
                        local_addr,
                        send_back_addr,
                        error,
                    } => {
                        warn!(
                            "Incoming connection error: {connection_id} {local_addr} {send_back_addr} {error}"
                        );
                    }
                    _ => {}
                };

                // Periodically print status updates
                if Instant::now() > next_check {
                    info!(
                        elapsed=?start_time.elapsed(),
                        remaining_connections=is_not_connected.len(),
                        subscriptions=subscriptions,
                        "connection establishment progress"
                    );
                    next_check = Instant::now() + connection_check_interval;
                }

                if is_not_connected.is_empty() {
                    info!("met all connection and subscription requirements");
                    return true;
                }
            }
            false
        };

        match timeout(general_timeout, connection_future).await {
            Ok(true) => {
                info!(elapsed=?start_time.elapsed(), "established all connections and subscriptions");
            }
            Ok(false) => {
                warn!(
                    elapsed=?start_time.elapsed(),
                    remaining_connections=is_not_connected.len(),
                    subscriptions=subscriptions,
                    "swarm event loop exited unexpectedly"
                );
            }
            Err(_) => {
                warn!(
                    elapsed=?start_time.elapsed(),
                    remaining_connections=is_not_connected.len(),
                    subscriptions=subscriptions,
                    "connection establishment timed out after {:?}", general_timeout
                );
            }
        }

        info!("established all connections and subscriptions");
    }

    /// Dials the given address and maps the resulting connection ID to the dial sequence ID.
    /// Used for both initial and retry dial attempts.
    async fn dial_and_map(&mut self, addr: Multiaddr, app_public_key: PublicKey) {
        let dial_opts = DialOpts::unknown_peer_id().address(addr.clone()).build();
        let conn_id = dial_opts.connection_id();
        self.dial_manager.map_connid(conn_id, app_public_key);
        match self.swarm.dial(dial_opts) {
            Ok(()) => info!(address = %addr, "Dialing libp2p peer"),
            Err(err) => error!(address = %addr, error = %err, "Could not connect to peer"),
        }
    }

    /// Starts listening and handling events from the network and commands from
    /// handles.
    ///
    /// # Implementation details
    ///
    /// This method should be spawned in separate async task or polled periodically
    /// to advance handling of new messages, event or commands.
    #[cfg_attr(
        not(all(feature = "gossipsub", feature = "request-response")),
        allow(unused_variables)
    )]
    pub async fn listen(mut self) {
        loop {
            let cancel_fut = self.cancellation_token.cancelled();
            let swarm_fut = self.swarm.select_next_some();
            let cmd_fut = self.commands.recv();

            #[cfg(feature = "gossipsub")]
            let gossip_fut = self.gossip_commands.recv();
            #[cfg(not(feature = "gossipsub"))]
            let gossip_fut = pending::<Option<()>>();

            #[cfg(feature = "request-response")]
            let reqresp_fut = self.request_response_commands.recv();
            #[cfg(not(feature = "request-response"))]
            let reqresp_fut = pending::<Option<()>>();

            let result = tokio::select! {
                _ = cancel_fut => {
                    debug!("Received cancellation, stopping listening");
                    return;
                }
                event = swarm_fut => {
                    self.handle_swarm_event(event).await
                }
                Some(cmd) = cmd_fut => {
                    self.handle_command(cmd).await
                }
                Some(gossip_cmd) = gossip_fut => {
                    #[cfg(feature = "gossipsub")]
                    {
                        self.handle_gossip_command(gossip_cmd).await
                    }
                    #[cfg(not(feature = "gossipsub"))]
                    {
                        unreachable!("gossip_fut never resolves when feature is disabled");
                        #[allow(unreachable_code)]
                        Ok(())
                    }
                }
                Some(req_cmd) = reqresp_fut => {
                    #[cfg(feature = "request-response")]
                    {
                        self.handle_request_response_command(req_cmd).await
                    }
                    #[cfg(not(feature = "request-response"))]
                    {
                        unreachable!("reqresp_fut never resolves when feature is disabled");
                        #[allow(unreachable_code)]
                        Ok(())
                    }
                }
            };

            if let Err(err) = result {
                error!(%err, "Stopping... encountered error...");
                return;
            }
        }
    }

    // Apply penalties for peer
    #[cfg(any(feature = "gossipsub", feature = "request-response"))]
    async fn apply_penalty(&mut self, app_public_key: &PublicKey, penalty: PenaltyType) {
        match penalty {
            PenaltyType::Ignore => (),
            PenaltyType::MuteGossip(time_amount) => {
                let until = SystemTime::now() + time_amount;
                match self
                    .peer_penalty_storage
                    .mute_peer_gossip(app_public_key, until)
                {
                    Ok(()) => info!(?app_public_key, ?until, "Peer muted for Gossipsub"),
                    Err(e) => error!(?app_public_key, ?e, "Failed to mute peer"),
                }
            }
            PenaltyType::MuteReqresp(time_amount) => {
                let until = SystemTime::now() + time_amount;
                match self
                    .peer_penalty_storage
                    .mute_peer_req_resp(app_public_key, until)
                {
                    Ok(()) => info!(?app_public_key, ?until, "Peer muted for RequestResponse"),
                    Err(e) => error!(?app_public_key, ?e, "Failed to mute peer"),
                }
            }
            PenaltyType::MuteBoth(time_amount) => {
                let until = SystemTime::now() + time_amount;
                let gossip_mute_result = self
                    .peer_penalty_storage
                    .mute_peer_gossip(app_public_key, until);
                let req_resp_mute_result = self
                    .peer_penalty_storage
                    .mute_peer_req_resp(app_public_key, until);
                match (gossip_mute_result, req_resp_mute_result) {
                    (Ok(()), Ok(())) => {
                        info!(
                            ?app_public_key,
                            ?until,
                            "Peer muted for both Gossipsub and RequestResponse"
                        )
                    }
                    (Err(e1), Err(e2)) => {
                        error!(
                            ?app_public_key,
                            ?e1,
                            ?e2,
                            "Failed to mute peer for both protocols"
                        )
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        error!(?app_public_key, ?e, "Failed to mute peer for one protocol")
                    }
                }
            }
            PenaltyType::Ban(opt_time_amount) => {
                let until = SystemTime::now() + opt_time_amount.unwrap_or(DEFAULT_BAN_PERIOD);
                match self.peer_penalty_storage.ban_peer(app_public_key, until) {
                    Ok(()) => {
                        info!(?app_public_key, ?until, "Peer banned");
                        let peer_id = self
                            .swarm
                            .behaviour()
                            .setup
                            .get_transport_id_by_application_key(app_public_key);
                        if let Some(peer_id) = peer_id {
                            let _ = self.swarm.disconnect_peer_id(peer_id);
                        } else {
                            warn!(?app_public_key, "No transport ID found for app public key");
                        }
                    }
                    Err(e) => error!(?app_public_key, ?e, "Failed to ban peer"),
                }
            }
        }
    }

    #[cfg(any(feature = "gossipsub", feature = "request-response"))]
    fn get_all_scores(&self, app_public_key: &PublicKey) -> PeerScore {
        #[cfg(feature = "gossipsub")]
        let (gossipsub_internal_score, gossipsub_app_score) = {
            match self
                .swarm
                .behaviour()
                .setup
                .get_transport_id_by_application_key(app_public_key)
            {
                Some(peer_id) => {
                    let internal_score = self
                        .swarm
                        .behaviour()
                        .gossipsub
                        .peer_score(&peer_id)
                        .unwrap_or(0.0);

                    let app_score = self
                        .score_manager
                        .get_gossipsub_app_score(app_public_key)
                        .unwrap_or(DEFAULT_GOSSIP_APP_SCORE);

                    (internal_score, app_score)
                }
                None => {
                    warn!(?app_public_key, "No transport ID found for app public key");
                    (0.0, 0.0)
                }
            }
        };

        #[cfg(feature = "request-response")]
        let req_resp_app_score = self
            .score_manager
            .get_req_resp_score(app_public_key)
            .unwrap_or(DEFAULT_REQ_RESP_APP_SCORE);

        PeerScore {
            #[cfg(feature = "gossipsub")]
            gossipsub_internal_score,
            #[cfg(feature = "gossipsub")]
            gossipsub_app_score,
            #[cfg(feature = "request-response")]
            req_resp_app_score,
        }
    }

    /// Handles a [`SwarmEvent`] from the swarm.
    async fn handle_swarm_event(
        &mut self,
        event: SwarmEvent<<Behaviour as NetworkBehaviour>::ToSwarm>,
    ) -> P2PResult<()> {
        match event {
            SwarmEvent::Behaviour(event) => self.handle_behaviour_event(event).await,
            SwarmEvent::ConnectionEstablished { .. }
            | SwarmEvent::OutgoingConnectionError { .. } => {
                self.handle_connection_event(event).await
            }
            _ => Ok(()),
        }
    }

    /// Handles connection-related events (both successful and failed connections).
    async fn handle_connection_event(
        &mut self,
        event: SwarmEvent<BehaviourEvent>,
    ) -> P2PResult<()> {
        match event {
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                ..
            } => {
                if let Some(app_public_key) = self
                    .dial_manager
                    .get_app_public_key_by_connection_id(&connection_id)
                {
                    self.dial_manager.remove_queue(&app_public_key);
                    self.dial_manager.remove_connid(&connection_id);
                    info!(peer_id = %peer_id, "connected to peer");
                }
                Ok(())
            }
            SwarmEvent::OutgoingConnectionError {
                connection_id,
                error,
                ..
            } => {
                if let Some(app_public_key) = self
                    .dial_manager
                    .get_app_public_key_by_connection_id(&connection_id)
                {
                    warn!(app_public_key = ?app_public_key, error = %error, "connection failed");
                    self.dial_manager.remove_connid(&connection_id);
                    if let Some(next_addr) = self.dial_manager.pop_next_addr(&app_public_key) {
                        info!(next_addr = %next_addr, app_public_key = ?app_public_key, "retrying with next address");
                        self.dial_and_map(next_addr, app_public_key).await;
                    } else {
                        warn!(app_public_key = ?app_public_key, "no more addresses to try, removing queue");
                        self.dial_manager.remove_queue(&app_public_key);
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Handles a [`BehaviourEvent`] from the swarm.
    async fn handle_behaviour_event(
        &mut self,
        event: <Behaviour as NetworkBehaviour>::ToSwarm,
    ) -> P2PResult<()> {
        match event {
            #[cfg(feature = "gossipsub")]
            BehaviourEvent::Gossipsub(event) => self.handle_gossip_event(event).await,
            #[cfg(feature = "request-response")]
            BehaviourEvent::RequestResponse(event) => {
                self.handle_request_response_event(event).await
            }
            behavior::BehaviourEvent::Setup(event) => self.handle_setup_event(event).await,
            _ => Ok(()),
        }
    }

    /// Handles a [`GossipsubEvent`] from the swarm.
    #[cfg(feature = "gossipsub")]
    async fn handle_gossip_event(&mut self, event: GossipsubEvent) -> P2PResult<()> {
        match event {
            GossipsubEvent::Message {
                propagation_source,
                message_id,
                message,
            } => {
                self.handle_gossip_msg(propagation_source, message_id, message)
                    .await
            }
            _ => Ok(()),
        }
    }

    /// Handles new message from gossipsub network.
    ///
    /// If message is not [`GossipsubMsg`] or is not signed, the message will
    /// be rejected without propagation, otherwise if wasn't handled before, send an [`Event`] to
    /// handles, store it and reset timeout.
    #[cfg(feature = "gossipsub")]
    #[instrument(skip(self, message), fields(sender = %message.source.unwrap()))]
    async fn handle_gossip_msg(
        &mut self,
        propagation_source: PeerId,
        message_id: MessageId,
        message: Message,
    ) -> P2PResult<()> {
        trace!(?message.data, "Got message");

        // Score/penalty logic for gossipsub
        let app_public_key = match self
            .swarm
            .behaviour()
            .setup
            .get_app_public_key_by_transport_id(&propagation_source)
        {
            Some(key) => key,
            None => {
                warn!(%propagation_source, "No app public key found for peer");
                return Ok(());
            }
        };

        if self.peer_penalty_storage.is_gossip_muted(&app_public_key) {
            warn!(
                %propagation_source, "Peer is muted for Gossipsub"
            );
            return Ok(());
        }

        let signed_message = match SignedMessage::from_json_bytes(&message.data) {
            Ok(signed_msg) => signed_msg,
            Err(e) => {
                error!(%propagation_source, ?e, "Failed to deserialize signed gossipsub message");
                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .report_message_validation_result(
                        &message_id,
                        &propagation_source,
                        MessageAcceptance::Reject,
                    );
                return Ok(());
            }
        };

        let gossip_message: GossipMessage = match signed_message.deserialize_message() {
            Ok(msg) => msg,
            Err(e) => {
                error!(%propagation_source, ?e, "Failed to deserialize inner gossipsub message");
                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .report_message_validation_result(
                        &message_id,
                        &propagation_source,
                        MessageAcceptance::Reject,
                    );
                return Ok(());
            }
        };

        let is_signature_valid =
            match signed_message.verify_signature(&gossip_message.app_public_key) {
                Ok(valid) => valid,
                Err(e) => {
                    error!(%propagation_source, ?e, "Error verifying gossipsub message signature");
                    self.swarm
                        .behaviour_mut()
                        .gossipsub
                        .report_message_validation_result(
                            &message_id,
                            &propagation_source,
                            MessageAcceptance::Reject,
                        );
                    return Ok(());
                }
            };

        if !is_signature_valid {
            warn!(%propagation_source, "Invalid signature for gossipsub message");
            self.swarm
                .behaviour_mut()
                .gossipsub
                .report_message_validation_result(
                    &message_id,
                    &propagation_source,
                    MessageAcceptance::Reject,
                );
            return Ok(());
        }

        if !self.allowlist.contains(&gossip_message.app_public_key) {
            warn!(%propagation_source, "Gossipsub message from peer not in allowlist");
            self.swarm
                .behaviour_mut()
                .gossipsub
                .report_message_validation_result(
                    &message_id,
                    &propagation_source,
                    MessageAcceptance::Reject,
                );
            return Ok(());
        }

        info!(%propagation_source, "Verified signed gossipsub message");

        let old_app_score = self
            .score_manager
            .get_gossipsub_app_score(&app_public_key)
            .unwrap_or(DEFAULT_GOSSIP_APP_SCORE);

        let updated_score = self.validator.validate_msg(
            &MessageType::Gossipsub(gossip_message.message.clone()),
            old_app_score,
        );

        self.score_manager
            .update_gossipsub_app_score(&app_public_key, updated_score);

        let PeerScore {
            gossipsub_app_score,
            req_resp_app_score,
            gossipsub_internal_score,
        } = self.get_all_scores(&app_public_key);

        if let Some(penalty) = self.validator.get_penalty(
            &MessageType::Gossipsub(gossip_message.message.clone()),
            gossipsub_internal_score,
            gossipsub_app_score,
            req_resp_app_score,
        ) {
            self.apply_penalty(&app_public_key, penalty).await;
            return Ok(());
        }

        self.swarm
            .behaviour_mut()
            .gossipsub
            .report_message_validation_result(
                &message_id,
                &propagation_source,
                MessageAcceptance::Accept,
            );

        // Send event to gossip_events channel with the actual message data
        self.gossip_events
            .send(GossipEvent::ReceivedMessage(gossip_message.message))
            .map_err(|e| ProtocolError::GossipEventsChannelClosed(e.into()))?;

        Ok(())
    }

    /// Handles command sent through channel by P2P implementation user.
    async fn handle_command(&mut self, cmd: Command) -> P2PResult<()> {
        match cmd {
            Command::ConnectToPeer {
                #[cfg(feature = "byos")]
                app_public_key,
                #[cfg(not(feature = "byos"))]
                transport_id,
                mut addresses,
            } => {
                if addresses.is_empty() {
                    warn!("No addresses provided to dial");
                    return Ok(());
                }

                #[cfg(feature = "byos")]
                {
                    if self.dial_manager.has_app_public_key(&app_public_key) {
                        error!(
                            "Already dialing peer with app_public_key: {:?}",
                            app_public_key
                        );
                        return Ok(());
                    }

                    addresses
                        .sort_by_key(|addr| !addr.protocol_stack().any(|proto| proto.contains("quic")));

                    let mut queue = Queue::from_iter(addresses.into_iter());

                    let first_addr = queue.pop_front().unwrap(); // can use unwrap() here thus we have at least one element

                    self.config.connect_to.push(first_addr.clone());
                    self.dial_manager
                        .insert_queue(app_public_key.clone(), queue);
                    self.dial_and_map(first_addr, app_public_key).await;
                }

                #[cfg(not(feature = "byos"))]
                {
                    if self.swarm.is_connected(&transport_id) {
                        error!(
                            "Already connected to peer with transport_id: {:?}",
                            transport_id
                        );
                        return Ok(());
                    }

                    addresses
                        .sort_by_key(|addr| !addr.protocol_stack().any(|proto| proto.contains("quic")));

                    // Build dial options with override_dial_concurrency_factor(1) to dial addresses sequentially
                    let dial_opts = DialOpts::peer_id(transport_id)
                        .addresses(addresses)
                        .override_dial_concurrency_factor(NonZeroU8::new(1).unwrap())
                        .build();

                    match self.swarm.dial(dial_opts) {
                        Ok(_) => {
                            debug!(%transport_id, "Initiated dial to peer with all provided addresses");
                        }
                        Err(e) => {
                            warn!(%transport_id, ?e, "Failed to dial peer");
                        }
                    }
                }

                Ok(())
            }
            Command::DisconnectFromPeer {
                #[cfg(feature = "byos")]
                target_app_public_key,
                #[cfg(not(feature = "byos"))]
                target_transport_id,
            } => {
                #[cfg(feature = "byos")]
                {
                    debug!(?target_app_public_key, "Got DisconnectFromPeer command");
                    let peer_id = target_app_public_key.to_peer_id();
                    if self.swarm.is_connected(&peer_id) {
                        let _ = self.swarm.disconnect_peer_id(peer_id);
                        debug!(%peer_id, "Initiated disconnect");
                    } else {
                        debug!(%peer_id, "Peer not connected, nothing to disconnect");
                    }
                }

                #[cfg(not(feature = "byos"))]
                {
                    debug!(?target_transport_id, "Got DisconnectFromPeer command");
                    if self.swarm.is_connected(&target_transport_id) {
                        let _ = self.swarm.disconnect_peer_id(target_transport_id);
                        debug!(%target_transport_id, "Initiated disconnect");
                    } else {
                        debug!(%target_transport_id, "Peer not connected, nothing to disconnect");
                    }
                }

                Ok(())
            }
            Command::QueryP2PState(query) => match query {
                QueryP2PStateCommand::IsConnected {
                    #[cfg(feature = "byos")]
                    app_public_key,
                    #[cfg(not(feature = "byos"))]
                    transport_id,
                    response_sender,
                } => {
                    #[cfg(feature = "byos")]
                    {
                        info!("Querying if app public key is connected");

                        let is_connected = match self
                            .swarm
                            .behaviour()
                            .setup
                            .get_transport_id_by_application_key(&app_public_key)
                        {
                            Some(transport_id) => {
                                info!(%transport_id, "Found transport ID for app public key");
                                self.swarm.is_connected(&transport_id)
                            }
                            None => {
                                info!("No transport ID found for app public key");
                                false
                            }
                        };

                        let _ = response_sender.send(is_connected);
                    }

                    #[cfg(not(feature = "byos"))]
                    {
                        info!("Querying if transport ID is connected");
                        let is_connected = self.swarm.is_connected(&transport_id);
                        let _ = response_sender.send(is_connected);
                    }

                    Ok(())
                }
                QueryP2PStateCommand::GetConnectedPeers { response_sender } => {
                    info!("Querying connected peers");
                    let peer_ids = self.swarm.connected_peers().cloned().collect::<Vec<_>>();

                    #[cfg(feature = "byos")]
                    {
                        let public_keys: Vec<PublicKey> = peer_ids
                            .into_iter()
                            .filter_map(|peer_id| {
                                self.swarm
                                    .behaviour()
                                    .setup
                                    .get_app_public_key_by_transport_id(&peer_id)
                            })
                            .collect();

                        let _ = response_sender.send(public_keys);
                    }

                    #[cfg(not(feature = "byos"))]
                    {
                        let _ = response_sender.send(peer_ids);
                    }

                    Ok(())
                }
                QueryP2PStateCommand::GetMyListeningAddresses { response_sender } => {
                    info!("Querying my own local listening addresses.");
                    // We clone here because if not clone, we'll receive `Vec<&Multiaddr>`. Ok,
                    // we'll receive Vec of references. Then in enum of `QueryP2PStateCommand`
                    // we'll have to specify template lifetime param. Then we'll have to specify it
                    // in Commands, then fix a lot of code to specify `<'_>`.
                    //
                    // For the case it seems more reasonable to clone than to struggle with
                    // lifetimes, since we don't expect this
                    // command be called many times.
                    let multiaddresses =
                        self.swarm.listeners().cloned().collect::<Vec<Multiaddr>>();
                    let _ = response_sender.send(multiaddresses);
                    Ok(())
                }
            },
        }
    }

    /// Handles gossip command sent through GossipHandle.
    #[cfg(feature = "gossipsub")]
    async fn handle_gossip_command(&mut self, cmd: GossipCommand) -> P2PResult<()> {
        debug!("Publishing message");
        trace!(?cmd.data, "Publishing message");

        let signed_gossip_message: SignedMessage = match SignedMessage::new_signed_gossip(
            self.config.app_public_key.clone(),
            cmd.data,
            self.signer.as_ref(),
        ) {
            Ok(signed_msg) => signed_msg,
            Err(e) => {
                error!(?e, "Failed to create signed gossipsub message");
                return Ok(());
            }
        };

        let signed_message_data = match serde_json::to_vec(&signed_gossip_message) {
            Ok(data) => data,
            Err(e) => {
                error!(?e, "Failed to serialize signed gossipsub message");
                return Ok(());
            }
        };

        let message_id = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .publish(TOPIC.hash(), signed_message_data)
            .inspect_err(|err| match err {
                PublishError::Duplicate => {
                    warn!(%err, "Failed to publish msg through gossipsub, message already exists");
                }
                PublishError::SigningError(signing_error) => {
                    error!(%signing_error, "Failed to sign message");
                }
                PublishError::InsufficientPeers => {
                    error!("Insufficient peers to publish message");
                }
                PublishError::MessageTooLarge => {
                    error!("Message too large to publish");
                }
                PublishError::TransformFailed(error) => {
                    error!(%error, "Failed to transform message");
                }
                PublishError::AllQueuesFull(num_peers_attempted) => {
                    error!(%num_peers_attempted, "All queues full, dropping message");
                }
            });

        if message_id.is_ok() {
            debug!(message_id=%message_id.unwrap(), "Message published");
        }

        Ok(())
    }

    /// Handles request-response command sent through ReqRespHandle.
    #[cfg(feature = "request-response")]
    async fn handle_request_response_command(
        &mut self,
        cmd: RequestResponseCommand,
    ) -> P2PResult<()> {
        // Extract target peer ID based on feature mode
        let target_peer_id = {
            #[cfg(feature = "byos")]
            {
                debug!(?cmd.target_app_public_key, "Got request message");
                self.swarm
                    .behaviour()
                    .setup
                    .get_transport_id_by_application_key(&cmd.target_app_public_key)
            }
            #[cfg(not(feature = "byos"))]
            {
                debug!(?cmd.target_transport_id, "Got request message");
                Some(cmd.target_transport_id)
            }
        };

        trace!(?cmd.data, "Got request message");

        // Create and serialize signed message
        let signed_request_message = match SignedMessage::new_signed_request(
            self.config.app_public_key.clone(),
            cmd.data,
            self.signer.as_ref(),
        ) {
            Ok(signed_msg) => signed_msg,
            Err(e) => {
                error!(?e, "Failed to create signed request message");
                return Ok(());
            }
        };

        let signed_request_message_data = match serde_json::to_vec(&signed_request_message) {
            Ok(data) => data,
            Err(e) => {
                error!(?e, "Failed to serialize signed request message");
                return Ok(());
            }
        };

        // Send request if peer is available and connected
        match target_peer_id {
            Some(peer_id) => {
                if self.swarm.is_connected(&peer_id) {
                    self.swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(&peer_id, signed_request_message_data);
                } else {
                    debug!(%peer_id, "Peer not connected, cannot send request");
                }
            }
            None => {
                #[cfg(feature = "byos")]
                error!("Logic error: Request response is attempted on a peer we haven't done Setup yet.");
            }
        }

        Ok(())
    }

    /// Handles [`RequestResponseEvent`] from the swarm.
    #[cfg(feature = "request-response")]
    #[instrument(skip(self, event))]
    async fn handle_request_response_event(
        &mut self,
        event: RequestResponseEvent<Vec<u8>, Vec<u8>>,
    ) -> P2PResult<()> {
        match event {
            RequestResponseEvent::Message { peer, message, .. } => {
                debug!(%peer, "Received message");
                trace!(%peer, ?message, "Received message");
                self.handle_message_event(peer, message).await?
            }
            RequestResponseEvent::OutboundFailure {
                peer,
                request_id,
                error,
                ..
            } => {
                error!(%peer, %error, %request_id, "Outbound failure")
            }
            RequestResponseEvent::InboundFailure {
                peer,
                request_id,
                error,
                connection_id: _,
            } => {
                error!(%peer, %error, %request_id, "Inbound failure");
                // dial the peer
                let _ = self.swarm.dial(peer).inspect_err(|err| {
                    error!(%peer, %error, %request_id, "Inbound failure");
                    error!(%err, "Failed to connect to peer '{peer}'");
                });
            }
            RequestResponseEvent::ResponseSent {
                peer, request_id, ..
            } => {
                debug!(%peer, %request_id, "Response sent")
            }
        }

        Ok(())
    }

    /// Handles [`MessageEvent`] from the swarm.
    #[cfg(feature = "request-response")]
    async fn handle_message_event(
        &mut self,
        peer_id: PeerId,
        msg: request_response::Message<Vec<u8>, Vec<u8>, Vec<u8>>,
    ) -> P2PResult<()> {
        let reqresp_timeout = self
            .config
            .channel_timeout
            .unwrap_or(DEFAULT_CHANNEL_TIMEOUT);
        let app_public_key = match self
            .swarm
            .behaviour()
            .setup
            .get_app_public_key_by_transport_id(&peer_id)
        {
            Some(key) => key,
            None => {
                warn!(%peer_id, "No app public key found for peer");
                return Ok(());
            }
        };
        match msg {
            request_response::Message::Request {
                request, channel, ..
            } => {
                // Score/penalty logic for request
                if self.peer_penalty_storage.is_req_resp_muted(&app_public_key) {
                    warn!(%peer_id, "Peer is muted for request/response");
                    return Ok(());
                }

                let signed_message = match SignedMessage::from_json_bytes(&request) {
                    Ok(signed_msg) => signed_msg,
                    Err(e) => {
                        error!(%peer_id, ?e, "Failed to deserialize signed request message");
                        return Ok(());
                    }
                };

                let request: RequestMessage = match signed_message.deserialize_message() {
                    Ok(request) => request,
                    Err(e) => {
                        error!(%peer_id, ?e, "Failed to deserialize request message");
                        return Ok(());
                    }
                };

                let is_valid = match signed_message.verify_signature(&request.app_public_key) {
                    Ok(is_valid) => is_valid,
                    Err(e) => {
                        error!(%peer_id, ?e, "Failed to verify signature for request message");
                        return Ok(());
                    }
                };

                if !is_valid {
                    error!(%peer_id, "Invalid signature for request message");
                    return Ok(());
                }

                let old_app_score = self
                    .score_manager
                    .get_req_resp_score(&app_public_key)
                    .unwrap_or(DEFAULT_REQ_RESP_APP_SCORE);

                let updated_score = self.validator.validate_msg(
                    &MessageType::Request(request.message.clone()),
                    old_app_score,
                );

                self.score_manager
                    .update_req_resp_app_score(&app_public_key, updated_score);

                let PeerScore {
                    gossipsub_internal_score,
                    gossipsub_app_score,
                    req_resp_app_score,
                } = self.get_all_scores(&app_public_key);

                if let Some(penalty) = self.validator.get_penalty(
                    &MessageType::Request(request.message.clone()),
                    gossipsub_internal_score,
                    gossipsub_app_score,
                    req_resp_app_score,
                ) {
                    self.apply_penalty(&app_public_key, penalty).await;
                    return Ok(());
                }

                let (tx, rx) = oneshot::channel();

                let event = ReqRespEvent::ReceivedRequest(request.message.clone(), tx);
                let send_result = timeout(reqresp_timeout, self.req_resp_events.send(event)).await;

                match send_result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        return Err(SwarmError::Protocol(
                            ProtocolError::ReqRespEventChannelClosed(e.into()),
                        ));
                    }
                    Err(_) => {
                        error!("Timeout while sending ReceivedRequest event");
                    }
                }

                let resp = timeout(reqresp_timeout, rx).await;

                let _ = match resp {
                    Ok(Ok(response)) => {
                        let signed_response_message: SignedMessage =
                            match SignedMessage::new_signed_response(
                                self.config.app_public_key.clone(),
                                response,
                                self.signer.as_ref(),
                            ) {
                                Ok(signed_msg) => signed_msg,
                                Err(e) => {
                                    error!(?e, "Failed to create signed response message");
                                    return Ok(());
                                }
                            };
                        let signed_response_message_data =
                            match serde_json::to_vec(&signed_response_message) {
                                Ok(data) => data,
                                Err(e) => {
                                    error!(?e, "Failed to serialize signed response message");
                                    return Ok(());
                                }
                            };
                        self
                                .swarm
                                .behaviour_mut()
                                .request_response
                                .send_response(channel, signed_response_message_data)
                                .map_err(|_| {
                                    error!("Failed to send response: connection dropped or response channel closed");
                                })
                    }
                    Ok(Err(err)) => {
                        error!("Received error in response: {err:?}");
                        Ok(())
                    }
                    Err(_) => {
                        error!("Timeout waiting for response to request");
                        Ok(())
                    }
                };
                Ok(())
            }
            request_response::Message::Response { response, .. } => {
                // Score/penalty logic for response
                if self.peer_penalty_storage.is_req_resp_muted(&app_public_key) {
                    warn!(%peer_id, "Peer is muted for request/response");
                    return Ok(());
                }

                let signed_response_message = match SignedMessage::from_json_bytes(&response) {
                    Ok(signed_msg) => signed_msg,
                    Err(e) => {
                        error!(%peer_id, ?e, "Failed to deserialize signed response message");
                        return Ok(());
                    }
                };

                let response: ResponseMessage = match signed_response_message.deserialize_message()
                {
                    Ok(response) => response,
                    Err(e) => {
                        error!(%peer_id, ?e, "Failed to deserialize response message");
                        return Ok(());
                    }
                };

                let is_valid =
                    match signed_response_message.verify_signature(&response.app_public_key) {
                        Ok(is_valid) => is_valid,
                        Err(e) => {
                            error!(%peer_id, ?e, "Failed to verify signature for response message");
                            return Ok(());
                        }
                    };

                if !is_valid {
                    warn!(%peer_id, "Invalid signature for response message");
                    return Ok(());
                }

                let old_app_score = self
                    .score_manager
                    .get_req_resp_score(&app_public_key)
                    .unwrap_or(DEFAULT_REQ_RESP_APP_SCORE);

                let updated_score = self.validator.validate_msg(
                    &MessageType::Response(response.message.clone()),
                    old_app_score,
                );

                self.score_manager
                    .update_req_resp_app_score(&app_public_key, updated_score);

                let PeerScore {
                    gossipsub_internal_score,
                    gossipsub_app_score,
                    req_resp_app_score,
                } = self.get_all_scores(&app_public_key);

                if let Some(penalty) = self.validator.get_penalty(
                    &MessageType::Response(response.message.clone()),
                    gossipsub_internal_score,
                    gossipsub_app_score,
                    req_resp_app_score,
                ) {
                    self.apply_penalty(&app_public_key, penalty).await;
                    return Ok(());
                }

                let event = ReqRespEvent::ReceivedResponse(response.message.clone());
                let send_result = timeout(reqresp_timeout, self.req_resp_events.send(event)).await;
                match send_result {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        return Err(SwarmError::Protocol(
                            ProtocolError::ReqRespEventChannelClosed(e.into()),
                        ));
                    }
                    Err(_) => {
                        error!("Timeout while sending ReceivedResponse event");
                    }
                }

                Ok(())
            }
        }
    }

    /// Handles a [`SetupEvent`] from the swarm.
    async fn handle_setup_event(&mut self, event: SetupBehaviourEvent) -> P2PResult<()> {
        match event {
            SetupBehaviourEvent::AppKeyReceived {
                transport_id: peer_id,
                app_public_key,
            } => {
                if self.peer_penalty_storage.is_banned(&app_public_key) {
                    info!(%peer_id, "Received app public key from banned peer. Disconnecting.");
                    let _ = self.swarm.disconnect_peer_id(peer_id);
                    return Ok(());
                }

                if self.allowlist.contains(&app_public_key) {
                    info!(%peer_id, "Received app public key from peer");
                    trace!(%peer_id, ?app_public_key, "App public key details");
                } else {
                    info!(%peer_id, "Received app public key from a peer with not application public key not in allowlist. Disconnecting.");
                    let _ = self.swarm.disconnect_peer_id(peer_id);
                }
            }
            SetupBehaviourEvent::ErrorDuringSetupHandshake {
                transport_id: peer_id,
                error,
            } => {
                warn!(%peer_id, ?error, "Error during SetupBehaviour's handshake, disconnecting peer");
                // Drop the connection
                if let Err(e) = self.swarm.disconnect_peer_id(peer_id) {
                    warn!(%peer_id, ?e, "Failed to disconnect peer after SetupBehaviour's handshake failure");
                }
            }
        }
        Ok(())
    }
}

/// Constructs swarm builder with existing identity.
///
/// # Implementation details
///
/// Macro is used here, as [`libp2p`] doesn't expose internal generic types of [`SwarmBuilder`] to
/// actually specify return type of function. So we use macro for now.
macro_rules! init_swarm {
    ($cfg:expr) => {
        SwarmBuilder::with_existing_identity($cfg.transport_keypair.clone().into()).with_tokio()
    };
}

/// Finishes builder of swarm with parameters from config.
///
/// # Implementation details
///
/// Macro is used as there is no way to specify this behaviour in function, because `with_tcp` and
/// `with_other_transport` return completely different types that can't be generalized.
macro_rules! finish_swarm {
    ($builder:expr, $cfg:expr, $signer:expr) => {
        $builder
            .map_err(|e| ProtocolError::TransportInitialization(e.into()))?
            .with_behaviour(|_| {
                Behaviour::new(
                    PROTOCOL_NAME,
                    &$cfg.transport_keypair,
                    &$cfg.app_public_key,
                    #[cfg(feature = "gossipsub")]
                    &$cfg.gossipsub_score_params,
                    #[cfg(feature = "gossipsub")]
                    &$cfg.gossipsub_score_thresholds,
                    $signer.clone(),
                )
                .map_err(|e| e.into())
            })
            .map_err(|e| ProtocolError::BehaviourInitialization(e.into()))?
            .with_swarm_config(|c| c.with_idle_connection_timeout($cfg.idle_connection_timeout))
            .build()
    };
}

/// Constructs swarm from P2P config with inmemory transport. Uses
/// `/memory/{n}` addresses.
pub fn with_inmemory_transport(
    config: &P2PConfig,
    signer: Arc<dyn ApplicationSigner>,
) -> P2PResult<Swarm<Behaviour>> {
    let builder = init_swarm!(config);
    let swarm = finish_swarm!(
        builder.with_other_transport(|our_keypair| {
            MemoryTransport::new()
                .upgrade(libp2p::core::upgrade::Version::V1)
                .authenticate(noise::Config::new(our_keypair).unwrap())
                .multiplex(yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        }),
        config,
        signer
    );

    Ok(swarm)
}

/// Constructs a `Swarm<Behaviour>` from `P2PConfig` using QUIC with TCP fallback when available, or
/// TCP only otherwise.
pub fn with_default_transport(
    config: &P2PConfig,
    signer: Arc<dyn ApplicationSigner>,
) -> P2PResult<Swarm<Behaviour>> {
    let builder = init_swarm!(config);
    #[cfg(feature = "quic")]
    let swarm = builder
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )
        .unwrap()
        .with_quic()
        .with_behaviour(|_| {
            Behaviour::new(
                PROTOCOL_NAME,
                &config.transport_keypair,
                &config.app_public_key,
                #[cfg(feature = "gossipsub")]
                &config.gossipsub_score_params,
                #[cfg(feature = "gossipsub")]
                &config.gossipsub_score_thresholds,
                signer.clone(),
            )
            .map_err(|e| e.into())
        })
        .map_err(|e| ProtocolError::BehaviourInitialization(e.into()))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(config.idle_connection_timeout))
        .build();
    #[cfg(not(feature = "quic"))]
    let swarm = finish_swarm!(
        builder.with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        ),
        config,
        signer
    );
    Ok(swarm)
}
