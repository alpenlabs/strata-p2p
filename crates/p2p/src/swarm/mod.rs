//! Swarm implementation for P2P.

#[cfg(any(feature = "gossipsub", feature = "request-response"))]
use std::time::SystemTime;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use behavior::{Behaviour, BehaviourEvent};
use cynosure::site_c::queue::Queue;
use errors::{P2PResult, ProtocolError};
use futures::StreamExt as _;
#[cfg(not(all(feature = "gossipsub", feature = "request-response")))]
use futures::future::pending;
use handle::CommandHandle;
use libp2p::{
    Multiaddr, PeerId, Swarm, SwarmBuilder, Transport,
    core::{ConnectedPoint, muxing::StreamMuxerBox, transport::MemoryTransport},
    gossipsub::{
        Event as GossipsubEvent, Message, MessageAcceptance, MessageId, PublishError, Sha256Topic,
    },
    identify,
    identity::{Keypair, PublicKey},
    noise,
    swarm::{NetworkBehaviour, SwarmEvent, dial_opts::DialOpts},
    tcp, yamux,
};
#[cfg(feature = "kad")]
use libp2p::{
    StreamProtocol,
    kad::{
        AddProviderError, AddProviderOk, BootstrapOk, Event as KademliaEvent, GetClosestPeersError,
        GetClosestPeersOk, GetProvidersError, PutRecordError, PutRecordOk, QueryResult,
    },
};
#[cfg(any(feature = "request-response", feature = "kad"))]
use tokio::sync::oneshot;
use tokio::{
    sync::{broadcast, mpsc},
    time::timeout,
};
use tokio_util::sync::CancellationToken;
#[cfg(any(feature = "gossipsub", feature = "request-response"))]
use tracing::instrument;
use tracing::{debug, error, info, trace, warn};
#[cfg(feature = "gossipsub")]
use {
    crate::{
        commands::GossipCommand, events::GossipEvent, score_manager::DEFAULT_GOSSIP_APP_SCORE,
        swarm::dto::gossipsub::GossipMessage,
    },
    handle::GossipHandle,
    libp2p::gossipsub::{PeerScoreParams, PeerScoreThresholds},
};
#[cfg(feature = "request-response")]
use {
    crate::{
        commands::RequestResponseCommand,
        events::ReqRespEvent,
        score_manager::DEFAULT_REQ_RESP_APP_SCORE,
        swarm::dto::request_response::{RequestMessage, ResponseMessage},
    },
    errors::SwarmError,
    handle::ReqRespHandle,
    libp2p::request_response::{self, Event as RequestResponseEvent},
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
    swarm::dto::signed::SignedMessage,
    validator::{DEFAULT_BAN_PERIOD, Message as MessageType, PenaltyType},
};

/// a non-exhaustive enum.
#[cfg(feature = "kad")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KadProtocol {
    /// first version of DHT
    V1,
}

#[cfg(feature = "kad")]
impl From<KadProtocol> for StreamProtocol {
    fn from(protocol: KadProtocol) -> Self {
        match protocol {
            KadProtocol::V1 => StreamProtocol::new("/kad/strata/0.0.1"),
        }
    }
}

mod behavior;
#[cfg(feature = "request-response")]
mod codec_raw;
pub mod dial_manager;
pub mod errors;
pub mod handle;

pub(crate) mod serializing;

pub mod dto;

pub mod setup;

/// Global topic name for gossipsub messages.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GossipSubTopic {
    /// second version
    V2,
}

impl From<GossipSubTopic> for Sha256Topic {
    fn from(protocol: GossipSubTopic) -> Self {
        match protocol {
            GossipSubTopic::V2 => Sha256Topic::new("bitvm3"),
        }
    }
}

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

    /// Kademlia protocol name
    #[cfg(feature = "kad")]
    pub kad_protocol_name: Option<KadProtocol>,

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
pub struct P2P<S, V = DefaultP2PValidator>
where
    S: ApplicationSigner,
    V: Validator,
{
    /// The swarm that handles the networking.
    swarm: Swarm<Behaviour<S>>,

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
    signer: S,

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
    validator: V,
}

/// Alias for [`P2P`] and [`ReqRespHandle`] tuple.
#[cfg(feature = "request-response")]
pub type P2PWithReqRespHandle<S, V> = (P2P<S, V>, ReqRespHandle);

impl<S, V> P2P<S, V>
where
    S: ApplicationSigner,
    V: Validator,
{
    /// Creates a new P2P instance from the given configuration.
    #[cfg(feature = "request-response")]
    pub fn from_config(
        cfg: P2PConfig,
        cancel: CancellationToken,
        mut swarm: Swarm<Behaviour<S>>,
        allowlist: Vec<PublicKey>,
        #[cfg(feature = "gossipsub")] channel_size: Option<usize>,
        signer: S,
        validator: Option<V>,
    ) -> P2PResult<P2PWithReqRespHandle<S, V>> {
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

        // Request-response setup
        let req_resp_event_buffer_size = cfg
            .req_resp_event_buffer_size
            .unwrap_or(DEFAULT_REQ_RESP_EVENT_BUFFER_SIZE);
        let (req_resp_event_tx, req_resp_event_rx) =
            mpsc::channel::<ReqRespEvent>(req_resp_event_buffer_size);
        let (req_resp_cmds_tx, req_resp_cmds_rx) = mpsc::channel(command_buffer_size);

        // Conditionally setup gossipsub channels
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

        let validator = validator.unwrap_or_default();

        Ok((
            P2P::<S, V> {
                swarm,
                #[cfg(feature = "gossipsub")]
                gossip_events: gossip_events_tx,
                req_resp_events: req_resp_event_tx,
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
                #[cfg(any(feature = "kad", feature = "request-response", feature = "gossipsub"))]
                signer,
                dial_manager: DialManager::new(),
                score_manager,
                peer_penalty_storage,
                validator,
            },
            #[cfg(feature = "request-response")]
            ReqRespHandle::new(req_resp_event_rx, req_resp_cmds_tx),
        ))
    }

    /// Creates a new P2P instance from the given configuration.
    #[cfg(not(feature = "request-response"))]
    pub fn from_config(
        cfg: P2PConfig,
        cancel: CancellationToken,
        mut swarm: Swarm<Behaviour<S>>,
        allowlist: Vec<PublicKey>,
        #[cfg(feature = "gossipsub")] channel_size: Option<usize>,
        signer: S,
        validator: Option<V>,
    ) -> P2PResult<P2P<S, V>> {
        for addr in &cfg.listening_addrs {
            swarm
                .listen_on(addr.clone())
                .map_err(ProtocolError::Listen)?;
        }

        // Gossipsub setup
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

        // Core components setup
        let (cmds_tx, cmds_rx, score_manager, peer_penalty_storage) = {
            let command_buffer_size = cfg
                .command_buffer_size
                .unwrap_or(DEFAULT_COMMAND_BUFFER_SIZE);
            let (cmds_tx, cmds_rx) = mpsc::channel(command_buffer_size);
            let score_manager = ScoreManager::new();
            let peer_penalty_storage = PenaltyPeerStorage::new();
            (cmds_tx, cmds_rx, score_manager, peer_penalty_storage)
        };

        let validator = validator.unwrap_or_default();

        Ok(P2P::<S, V> {
            swarm,
            #[cfg(feature = "gossipsub")]
            gossip_events: gossip_events_tx,
            #[cfg(feature = "gossipsub")]
            gossip_commands: gossip_cmds_rx,
            #[cfg(feature = "gossipsub")]
            gossip_commands_sender: gossip_cmds_tx.clone(),
            commands: cmds_rx,
            commands_sender: cmds_tx.clone(),
            cancellation_token: cancel,
            config: cfg,
            allowlist: HashSet::from_iter(allowlist),
            signer,
            dial_manager: DialManager::new(),
            score_manager,
            peer_penalty_storage,
            validator,
        })
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

        if !&self.config.connect_to.is_empty() {
            trace!(
                "Will try connect to next peers: {:?}",
                &self.config.connect_to
            );
            for multiaddr in &self.config.connect_to {
                loop {
                    if false {
                        break;
                    }
                    let mut num_retries = 0;
                    let some_dial_opts = DialOpts::unknown_peer_id()
                        .address(multiaddr.clone())
                        .build();
                    match self.swarm.dial(some_dial_opts) {
                        Ok(()) => {
                            info!(%multiaddr, "dialed peer");
                            break;
                        }

                        Err(err) => {
                            warn!(%err, %multiaddr, %num_retries, %max_retry_count, "failed to connect to peer, retrying...");
                        }
                    }

                    // Add a small delay between retries to avoid overwhelming the network
                    tokio::time::sleep(dial_timeout).await;

                    debug!(%multiaddr, %num_retries, %max_retry_count, "attempting to dial peer again");

                    num_retries += 1;

                    if num_retries > max_retry_count {
                        error!(%multiaddr, %num_retries, %max_retry_count, "failed to connect to peer after max retries");
                        is_not_connected.insert(multiaddr);
                        break;
                    }
                }
            }
        }

        #[cfg(feature = "gossipsub")]
        {
            let mut num_retries = 0;
            loop {
                debug!(topic=%Into::<Sha256Topic>::into(GossipSubTopic::V2).to_string(), %num_retries, %max_retry_count, "attempting to subscribe to topic");
                match timeout(general_timeout, async {
                    self.swarm
                        .behaviour_mut()
                        .gossipsub
                        .subscribe(&Into::<Sha256Topic>::into(GossipSubTopic::V2))
                })
                .await
                {
                    Ok(Ok(_)) => {
                        info!(topic=%Into::<Sha256Topic>::into(GossipSubTopic::V2).to_string(), %num_retries, %max_retry_count, local_addr=?self.config.listening_addrs, "subscribed to topic successfully");
                        break;
                    }
                    Ok(Err(err)) => {
                        error!(topic=%Into::<Sha256Topic>::into(GossipSubTopic::V2).to_string(), %err, %num_retries, %max_retry_count, local_addr=?self.config.listening_addrs, "failed to subscribe to topic, retrying...");
                    }
                    Err(_) => {
                        error!(topic=%Into::<Sha256Topic>::into(GossipSubTopic::V2).to_string(), %num_retries, %max_retry_count, local_addr=?self.config.listening_addrs, "failed to subscribe to topic, retrying...");
                    }
                }

                num_retries += 1;

                if num_retries > max_retry_count {
                    error!(topic=%Into::<Sha256Topic>::into(GossipSubTopic::V2).to_string(), %num_retries, %max_retry_count, "failed to subscribe to topic after max retries");
                    break;
                }

                // Add a small delay between retries
                tokio::time::sleep(connection_check_interval).await;
                debug!(topic=%Into::<Sha256Topic>::into(GossipSubTopic::V2).to_string(), %num_retries, %max_retry_count, "attempting to subscribe to topic again");
            }
            debug!(topic=%Into::<Sha256Topic>::into(GossipSubTopic::V2).to_string(), %num_retries, %max_retry_count, "finished trying to subscribe to topic");
        }

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
                        debug!(%peer_id, "got subscription from a peer");
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
                        "connection establishment progress"
                    );
                    next_check = Instant::now() + connection_check_interval;
                }

                if is_not_connected.is_empty() {
                    info!("met all connection requirements");
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
                    "swarm event loop exited unexpectedly"
                );
            }
            Err(_) => {
                warn!(
                    elapsed=?start_time.elapsed(),
                    remaining_connections=is_not_connected.len(),
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
                error!(%err, "Stopping P2P node : encountered error after which we think we can't continue operating or P2P node has been cancelled via cancellation token.");
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
        event: SwarmEvent<<Behaviour<S> as NetworkBehaviour>::ToSwarm>,
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
        event: SwarmEvent<BehaviourEvent<S>>,
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
        event: <Behaviour<S> as NetworkBehaviour>::ToSwarm,
    ) -> P2PResult<()> {
        match event {
            #[cfg(feature = "gossipsub")]
            BehaviourEvent::Gossipsub(event) => self.handle_gossip_event(event).await,
            #[cfg(feature = "request-response")]
            BehaviourEvent::RequestResponse(event) => {
                self.handle_request_response_event(event).await
            }
            BehaviourEvent::Setup(event) => self.handle_setup_event(event).await,
            #[cfg(feature = "kad")]
            BehaviourEvent::Kademlia(event) => self.handle_kademlia_event(event).await,
            BehaviourEvent::Identify(event) => self.handle_identify_event(event).await,
        }
    }

    async fn handle_identify_event(&mut self, event: identify::Event) -> P2PResult<()> {
        match event {
            identify::Event::Received {
                connection_id,
                peer_id,
                info,
            } => {
                trace!(?connection_id, %peer_id, ?info, "identify::Event::Received");
                #[cfg(feature = "kad")]
                if !info.listen_addrs.is_empty() {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, info.listen_addrs[0].clone());
                }
            }
            identify::Event::Sent {
                connection_id,
                peer_id,
            } => {
                trace!(?connection_id, %peer_id, "identify::Event::Sent");
            }
            identify::Event::Pushed {
                connection_id,
                peer_id,
                info,
            } => {
                trace!(?connection_id, %peer_id, ?info, "identify::Event::Pushed");
            }
            identify::Event::Error {
                connection_id,
                peer_id,
                error,
            } => {
                trace!(?connection_id, %peer_id, %error, "identify::Event::Error");
            }
        };
        Ok(())
    }

    /// Handles a [`KademliaEvent`] from the swarm.
    #[cfg(feature = "kad")]
    async fn handle_kademlia_event(&mut self, event: KademliaEvent) -> P2PResult<()> {
        match event {
            KademliaEvent::InboundRequest { request } => match request {
                libp2p::kad::InboundRequest::FindNode { num_closer_peers } => {
                    trace!(%num_closer_peers, "InboundRequest::FindNode");
                    Ok(())
                }
                libp2p::kad::InboundRequest::PutRecord {
                    source,
                    connection,
                    record: opt_record,
                } => {
                    trace!(
                        %source, %connection, ?opt_record, "InboundRequest::PutRecord"
                    );
                    Ok(())
                }
                libp2p::kad::InboundRequest::GetRecord {
                    num_closer_peers,
                    present_locally,
                } => {
                    trace!(
                        %num_closer_peers, %present_locally, "libp2p::kad::InboundRequest::GetRecord"
                    );
                    Ok(())
                }
                libp2p::kad::InboundRequest::GetProvider {
                    num_closer_peers,
                    num_provider_peers,
                } => {
                    trace!(
                        %num_closer_peers, %num_provider_peers, "libp2p::kad::InboundRequest::GetProvider"
                    );
                    Ok(())
                }
                libp2p::kad::InboundRequest::AddProvider { record } => {
                    trace!(?record, "libp2p::kad::InboundRequest::AddProvider");
                    Ok(())
                }
            },

            KademliaEvent::OutboundQueryProgressed {
                id,
                result,
                stats,
                step,
            } => match result {
                QueryResult::Bootstrap(Ok(BootstrapOk {
                    peer,
                    num_remaining,
                })) => {
                    trace!(
                        %id, ?stats, ?step, %peer, %num_remaining, "QueryResult::Bootstrap(Ok(BootstrapOk "
                    );
                    Ok(())
                }
                QueryResult::Bootstrap(Err(libp2p::kad::BootstrapError::Timeout {
                    peer,
                    num_remaining,
                })) => {
                    trace!(
                        %id, ?stats, ?step, %peer, ?num_remaining, "QueryResult::Bootstrap(Err(BootstrapErr "
                    );
                    Ok(())
                }
                QueryResult::GetRecord(Ok(libp2p::kad::GetRecordOk::FoundRecord(peer_record))) => {
                    trace!(
                        %id, ?stats, ?step, peer_key = ?peer_record.record.key, peer_record = %String::from_utf8_lossy(&peer_record.record.value), "QueryResult::GetRecord(Ok(libp2p::kad::GetRecordOk::FoundRecord"
                    );
                    Ok(())
                }
                QueryResult::GetRecord(Ok(
                    libp2p::kad::GetRecordOk::FinishedWithNoAdditionalRecord { cache_candidates },
                )) => {
                    trace!(
                        %id, ?stats, ?step, ?cache_candidates, "QueryResult::GetRecord(Ok(libp2p::kad::GetRecordOk::FinishedWithNoAdditionalRecord"
                    );
                    Ok(())
                }
                QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::Timeout { key })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, "QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::Timeout"
                    );
                    Ok(())
                }
                QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::NotFound {
                    key,
                    closest_peers,
                })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, ?closest_peers, "QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::NotFound"
                    );
                    Ok(())
                }
                QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::QuorumFailed {
                    key,
                    records,
                    quorum,
                })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, ?records, %quorum, "QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::QuorumFailed"
                    );
                    Ok(())
                }
                QueryResult::PutRecord(Ok(PutRecordOk { key })) => {
                    trace!(%id, ?stats, ?step, ?key, "QueryResult::PutRecord(Ok(PutRecordOk");
                    Ok(())
                }
                QueryResult::PutRecord(Err(PutRecordError::QuorumFailed {
                    key,
                    success: success_peerids,
                    quorum,
                })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, ?success_peerids, %quorum, "QueryResult::PutRecord(Err(PutRecordError::QuorumFailed"
                    );
                    Ok(())
                }
                QueryResult::PutRecord(Err(PutRecordError::Timeout {
                    key,
                    success: success_peerids,
                    quorum,
                })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, ?success_peerids, %quorum, "QueryResult::PutRecord(Err(PutRecordError::Timeout"
                    );
                    Ok(())
                }
                QueryResult::GetProviders(Ok(libp2p::kad::GetProvidersOk::FoundProviders {
                    key,
                    providers,
                })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, ?providers, "QueryResult::GetProviders(Ok(libp2p::kad::GetProvidersOk::FoundProviders"
                    );
                    Ok(())
                }
                QueryResult::GetProviders(Ok(
                    libp2p::kad::GetProvidersOk::FinishedWithNoAdditionalRecord { closest_peers },
                )) => {
                    trace!(
                        %id, ?stats, ?step, ?closest_peers, "QueryResult::GetProviders(Ok(libp2p::kad::GetProvidersOk::FinishedWithNoAdditionalRecord"
                    );
                    Ok(())
                }
                QueryResult::GetProviders(Err(GetProvidersError::Timeout {
                    key,
                    closest_peers,
                })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, ?closest_peers, "QueryResult::GetProviders(Err(GetProvidersError::Timeout"
                    );
                    Ok(())
                }
                QueryResult::GetClosestPeers(Ok(GetClosestPeersOk { key, peers })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, ?peers, "QueryResult::GetClosestPeers(Ok(GetClosestPeersOk"
                    );
                    // TODO(Arniiiii) : since we manually add peers, here's a filtering could be
                    // implemented
                    for peer in peers {
                        self.swarm
                            .add_peer_address(peer.peer_id, peer.addrs[0].clone());
                    }
                    Ok(())
                }
                QueryResult::GetClosestPeers(Err(GetClosestPeersError::Timeout { key, peers })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, ?peers, "QueryResult::GetClosestPeers(Ok(GetClosestPeersOk"
                    );
                    Ok(())
                }
                QueryResult::StartProviding(Ok(AddProviderOk { key })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, "QueryResult::StartProviding(Ok(AddProviderOk"
                    );
                    Ok(())
                }
                QueryResult::StartProviding(Err(AddProviderError::Timeout { key })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, "QueryResult::StartProviding(Err(AddProviderError::Timeout"
                    );
                    Ok(())
                }
                QueryResult::RepublishRecord(Ok(PutRecordOk { key })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, "QueryResult::RepublishRecord(Ok(PutRecordOk"
                    );
                    Ok(())
                }
                QueryResult::RepublishRecord(Err(PutRecordError::QuorumFailed {
                    key,
                    success,
                    quorum,
                })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, ?success, ?quorum, "QueryResult::RepublishRecord(Err(PutRecordError::QuorumFailed"
                    );
                    Ok(())
                }
                QueryResult::RepublishRecord(Err(PutRecordError::Timeout {
                    key,
                    success,
                    quorum,
                })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, ?success, %quorum, "QueryResult::RepublishRecord(Err(PutRecordError::QuorumFailed"
                    );
                    Ok(())
                }
                QueryResult::RepublishProvider(Ok(AddProviderOk { key })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, "QueryResult::RepublishProvider(Ok(AddProviderOk"
                    );
                    Ok(())
                }
                QueryResult::RepublishProvider(Err(AddProviderError::Timeout { key })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, "QueryResult::RepublishProvider(Err(AddProviderError::Timeout"
                    );
                    Ok(())
                }
            },

            KademliaEvent::RoutingUpdated {
                peer,
                is_new_peer,
                addresses,
                bucket_range,
                old_peer,
            } => {
                trace!(
                    ?peer,
                    ?is_new_peer,
                    ?addresses,
                    ?bucket_range,
                    ?old_peer,
                    "KademliaEvent::RoutingUpdated"
                );
                Ok(())
            }
            KademliaEvent::RoutablePeer { peer, address } => {
                trace!(?peer, ?address, "KademliaEvent::RoutablePeer");
                Ok(())
            }
            KademliaEvent::UnroutablePeer { peer } => {
                trace!(?peer, "KademliaEvent::UnroutablePeer");
                Ok(())
            }

            KademliaEvent::PendingRoutablePeer { peer, address } => {
                trace!(?peer, ?address, "KademliaEvent::PendingRoutablePeer");
                Ok(())
            }
            KademliaEvent::ModeChanged { new_mode } => {
                trace!(?new_mode, "KademliaEvent::ModeChanged");
                Ok(())
            }
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
        use crate::swarm::dto::gossipsub::SignedGossipsubMessage;

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

        let signed_gossipsub_message: SignedGossipsubMessage = match serde_json::from_slice(
            &message.data,
        ) {
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

        if !signed_gossipsub_message.message.app_public_key.verify(
            &serde_json::to_vec(&signed_gossipsub_message.message).unwrap(),
            &signed_gossipsub_message.signature,
        ) {
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
        };

        if !self
            .allowlist
            .contains(&signed_gossipsub_message.message.app_public_key)
        {
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
            &MessageType::Gossipsub(signed_gossipsub_message.message.message.clone()),
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
            &MessageType::Gossipsub(signed_gossipsub_message.message.message.clone()),
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
            .send(GossipEvent::ReceivedMessage(
                signed_gossipsub_message.message.message,
            ))
            .map_err(|e| ProtocolError::GossipEventsChannelClosed(e.into()))?;

        Ok(())
    }

    /// Handles command sent through channel by P2P implementation user.
    async fn handle_command(&mut self, cmd: Command) -> P2PResult<()> {
        match cmd {
            Command::ConnectToPeer {
                app_public_key,
                mut addresses,
            } => {
                if addresses.is_empty() {
                    warn!("No addresses provided to dial");
                    return Ok(());
                }

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

                Ok(())
            }
            Command::DisconnectFromPeer {
                target_app_public_key,
            } => {
                debug!(?target_app_public_key, "Got DisconnectFromPeer command");
                let peer_id = target_app_public_key.to_peer_id();
                if self.swarm.is_connected(&peer_id) {
                    let _ = self.swarm.disconnect_peer_id(peer_id);
                    debug!(%peer_id, "Initiated disconnect");
                } else {
                    debug!(%peer_id, "Peer not connected, nothing to disconnect");
                }

                Ok(())
            }
            Command::QueryP2PState(query) => match query {
                QueryP2PStateCommand::IsConnected {
                    app_public_key,
                    response_sender,
                } => {
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
                    Ok(())
                }
                QueryP2PStateCommand::GetConnectedPeers { response_sender } => {
                    info!("Querying connected peers");
                    let peer_ids = self.swarm.connected_peers().cloned().collect::<Vec<_>>();

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
        use crate::swarm::dto::gossipsub::SignedGossipsubMessage;

        debug!("Publishing message");
        trace!(?cmd.data, "Publishing message");

        let signed_gossip_message: SignedGossipsubMessage = match SignedMessage::new(
            GossipMessage::new(self.config.app_public_key.clone(), cmd.data),
            &self.signer,
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
            .publish(
                Into::<Sha256Topic>::into(GossipSubTopic::V2).hash(),
                signed_message_data,
            )
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
        debug!(?cmd.target_app_public_key, "Got request message");
        trace!(?cmd.data, "Got request message");

        let option_request_target_peer_id = self
            .swarm
            .behaviour()
            .setup
            .get_transport_id_by_application_key(&cmd.target_app_public_key);

        let signed_request_message: SignedMessage<RequestMessage> = match SignedMessage::new(
            RequestMessage::new(self.config.app_public_key.clone(), cmd.data),
            &self.signer,
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

        match option_request_target_peer_id {
            Some(request_target_peer_id) => {
                if self.swarm.is_connected(&request_target_peer_id) {
                    self.swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(&request_target_peer_id, signed_request_message_data);
                    return Ok(());
                }
            }
            None => {
                error!(
                    "Logic error: Request response is attempted on a peer we haven't done Setup yet."
                )
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

                use crate::swarm::dto::request_response::SignedRequestMessage;
                if self.peer_penalty_storage.is_req_resp_muted(&app_public_key) {
                    warn!(%peer_id, "Peer is muted for request/response");
                    return Ok(());
                }

                let signed_request_message: SignedRequestMessage =
                    match serde_json::from_slice(&request) {
                        Ok(signed_msg) => signed_msg,
                        Err(e) => {
                            error!(%peer_id, ?e, "Failed to deserialize signed request message");
                            return Ok(());
                        }
                    };

                if !signed_request_message.message.app_public_key.verify(
                    &serde_json::to_vec(&signed_request_message.message).unwrap(),
                    &signed_request_message.signature,
                ) {
                    error!(%peer_id, "Invalid signature in received request message");
                    return Ok(());
                };

                let old_app_score = self
                    .score_manager
                    .get_req_resp_score(&app_public_key)
                    .unwrap_or(DEFAULT_REQ_RESP_APP_SCORE);

                let updated_score = self.validator.validate_msg(
                    &MessageType::Request(signed_request_message.message.message.clone()),
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
                    &MessageType::Request(signed_request_message.message.message.clone()),
                    gossipsub_internal_score,
                    gossipsub_app_score,
                    req_resp_app_score,
                ) {
                    self.apply_penalty(&app_public_key, penalty).await;
                    return Ok(());
                }

                let (tx, rx) = oneshot::channel();

                let event = ReqRespEvent::ReceivedRequest(
                    signed_request_message.message.message.clone(),
                    tx,
                );
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
                        use crate::swarm::dto::request_response::SignedResponseMessage;

                        let signed_response_message: SignedResponseMessage =
                            match SignedMessage::new(
                                ResponseMessage::new(self.config.app_public_key.clone(), response),
                                &self.signer,
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

                use crate::swarm::dto::request_response::SignedResponseMessage;
                if self.peer_penalty_storage.is_req_resp_muted(&app_public_key) {
                    warn!(%peer_id, "Peer is muted for request/response");
                    return Ok(());
                }

                let signed_response_message: SignedResponseMessage =
                    match serde_json::from_slice(&response) {
                        Ok(signed_msg) => signed_msg,
                        Err(e) => {
                            error!(%peer_id, ?e, "Failed to deserialize signed response message");
                            return Ok(());
                        }
                    };

                if !signed_response_message.message.app_public_key.verify(
                    &serde_json::to_vec(&signed_response_message.message).unwrap(),
                    &signed_response_message.signature,
                ) {
                    error!(%peer_id, "Invalid signature in received response message");
                    return Ok(());
                };

                let old_app_score = self
                    .score_manager
                    .get_req_resp_score(&app_public_key)
                    .unwrap_or(DEFAULT_REQ_RESP_APP_SCORE);

                let updated_score = self.validator.validate_msg(
                    &MessageType::Response(signed_response_message.message.message.clone()),
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
                    &MessageType::Response(signed_response_message.message.message.clone()),
                    gossipsub_internal_score,
                    gossipsub_app_score,
                    req_resp_app_score,
                ) {
                    self.apply_penalty(&app_public_key, penalty).await;
                    return Ok(());
                }

                let event =
                    ReqRespEvent::ReceivedResponse(signed_response_message.message.message.clone());
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
                    #[cfg(feature = "kad")]
                    &$cfg.kad_protocol_name,
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
pub fn with_inmemory_transport<S: ApplicationSigner>(
    config: &P2PConfig,
    signer: S,
) -> P2PResult<Swarm<Behaviour<S>>> {
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
pub fn with_default_transport<S: ApplicationSigner>(
    config: &P2PConfig,
    signer: S,
) -> P2PResult<Swarm<Behaviour<S>>> {
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
                #[cfg(feature = "kad")]
                &config.kad_protocol_name,
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
