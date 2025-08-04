//! Swarm implementation for P2P.

#[cfg(feature = "kad")]
use std::collections::HashMap;
use std::{
    collections::{HashSet, VecDeque},
    sync::LazyLock,
    time::{Duration, Instant},
};

use behavior::{Behaviour, BehaviourEvent};
use errors::{P2PResult, ProtocolError};
use futures::StreamExt as _;
use handle::CommandHandle;
#[cfg(feature = "gossipsub")]
use handle::GossipHandle;
#[cfg(feature = "request-response")]
use handle::ReqRespHandle;
#[cfg(feature = "request-response")]
use libp2p::request_response::{self, Event as RequestResponseEvent};
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
        GetClosestPeersOk, GetProvidersError, PutRecordError, PutRecordOk, QueryId, QueryResult,
        Quorum, Record, RecordKey, store::RecordStore,
    },
};
#[cfg(any(feature = "request-response", feature = "kad"))]
use tokio::sync::oneshot;
use tokio::{
    sync::{broadcast, mpsc},
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, trace, warn};

#[cfg(feature = "request-response")]
use crate::commands::RequestResponseCommand;
#[cfg(feature = "request-response")]
use crate::events::ReqRespEvent;
#[cfg(feature = "kad")]
use crate::swarm::dto::{dht_record::RecordData, dht_record::SignedRecord};
#[cfg(feature = "gossipsub")]
use crate::{commands::GossipCommand, events::GossipEvent};
use crate::{
    commands::{Command, QueryP2PStateCommand},
    signer::ApplicationSigner,
    swarm::{dial_manager::DialManager, setup::events::SetupBehaviourEvent},
};

/// a non exhaustive enum.
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
static TOPIC: LazyLock<Sha256Topic> = LazyLock::new(|| Sha256Topic::new("bitvm2"));

/// Global MAX_TRANSMIT_SIZE for gossipsub messages.
const MAX_TRANSMIT_SIZE: usize = 512 * 1024;

/// Global name of the protocol
// TODO(Velnbur): make this configurable later
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

#[cfg(feature = "kad")]
/// What to do when we got event [`libp2p::kad::GetRecordOk`].
enum ActionOnKademliaGetRecord {
    /// Action related to [`Command::GetDHTRecord`].
    JustThrowResultBack {
        /// `tx` that we got from [`Command::GetDHTRecord`].
        tx: oneshot::Sender<Option<SignedRecord>>,
    },
}

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

    /// How many peers we should connect to before trying to put our signed [`RecordData`] for app
    /// public key
    #[cfg(feature = "kad")]
    pub kademlia_threshold: usize,

    /// Timeout for channel operations (e.g., sending/receiving on channels).
    #[cfg(feature = "request-response")]
    pub channel_timeout: Option<Duration>,
}

/// Implementation of P2P protocol data exchange.
#[expect(missing_debug_implementations)]
pub struct P2P<S: ApplicationSigner> {
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
    #[cfg(any(feature = "kad", feature = "request-response", feature = "gossipsub"))]
    signer: S,

    /// It is used to put record only if we connected to at least
    /// [`P2PConfig::kademlia_threshold`].
    #[cfg(feature = "kad")]
    kademlia_is_initial_record_already_posted: bool,

    /// Postponed action: a channel for a query to which send something back.
    #[cfg(feature = "kad")]
    kademlia_postponed_get_action: HashMap<QueryId, ActionOnKademliaGetRecord>,

    /// Manages dial sequences and address queues for multiaddress connections.
    dial_manager: DialManager,
}

/// Alias for [`P2P`] and [`ReqRespHandle`] tuple.
#[cfg(feature = "request-response")]
pub type P2PWithReqRespHandle<S> = (P2P<S>, ReqRespHandle);

impl<S: ApplicationSigner> P2P<S> {
    /// Creates a new P2P instance from the given configuration.
    #[cfg(feature = "request-response")]
    pub fn from_config(
        cfg: P2PConfig,
        cancel: CancellationToken,
        mut swarm: Swarm<Behaviour<S>>,
        allowlist: Vec<PublicKey>,
        channel_size: Option<usize>,
        signer: S,
    ) -> P2PResult<P2PWithReqRespHandle<S>> {
        for addr in &cfg.listening_addrs {
            swarm
                .listen_on(addr.clone())
                .map_err(ProtocolError::Listen)?;
        }

        let channel_size = channel_size.unwrap_or(256);
        let (cmds_tx, cmds_rx) = mpsc::channel(64);

        // Request-response setup
        let (req_resp_event_tx, req_resp_event_rx) = mpsc::channel(64);
        let (req_resp_cmds_tx, req_resp_cmds_rx) = mpsc::channel(64);

        // Conditionally setup gossipsub channels
        #[cfg(feature = "gossipsub")]
        let (gossip_events_tx, _gossip_events_rx) = broadcast::channel(channel_size);
        #[cfg(feature = "gossipsub")]
        let (gossip_cmds_tx, gossip_cmds_rx) = mpsc::channel(64);

        Ok((
            Self {
                swarm,

                // Request-response fields
                req_resp_events: req_resp_event_tx,
                request_response_commands: req_resp_cmds_rx,
                // Conditional gossipsub fields
                #[cfg(feature = "gossipsub")]
                gossip_events: gossip_events_tx,
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
                #[cfg(feature = "kad")]
                kademlia_is_initial_record_already_posted: false,
                #[cfg(feature = "kad")]
                kademlia_postponed_get_action: HashMap::new(),
                dial_manager: DialManager::new(),
            },
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
        channel_size: Option<usize>,
        signer: S,
    ) -> P2PResult<P2P> {
        for addr in &cfg.listening_addrs {
            swarm
                .listen_on(addr.clone())
                .map_err(ProtocolError::Listen)?;
        }

        let channel_size = channel_size.unwrap_or(256);
        let (cmds_tx, cmds_rx) = mpsc::channel(64);

        // Conditionally setup gossipsub channels
        #[cfg(feature = "gossipsub")]
        let (gossip_events_tx, _gossip_events_rx) = broadcast::channel(channel_size);
        #[cfg(feature = "gossipsub")]
        let (gossip_cmds_tx, gossip_cmds_rx) = mpsc::channel(64);

        Ok(Self {
            swarm,
            commands: cmds_rx,
            commands_sender: cmds_tx.clone(),

            // Conditional gossipsub fields
            #[cfg(feature = "gossipsub")]
            gossip_events: gossip_events_tx,
            #[cfg(feature = "gossipsub")]
            gossip_commands: gossip_cmds_rx,
            #[cfg(feature = "gossipsub")]
            gossip_commands_sender: gossip_cmds_tx.clone(),

            cancellation_token: cancel,
            config: cfg,
            allowlist: HashSet::from_iter(allowlist),
            signer,
            #[cfg(feature = "kad")]
            kademlia_is_initial_record_already_posted: false,
            #[cfg(feature = "kad")]
            kademlia_postponed_get_action: HashMap::new(),
            addr_store: SharedAddrStore(Arc::new(Mutex::new(HashMap::new()))),
            dial_manager: DialManager::new(),
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

        let mut num_retries = 0;
        loop {
            debug!(topic=%TOPIC.to_string(), %num_retries, %max_retry_count, "attempting to subscribe to topic");
            match timeout(general_timeout, async {
                self.swarm.behaviour_mut().gossipsub.subscribe(&TOPIC)
            })
            .await
            {
                Ok(Ok(_)) => {
                    info!(topic=%TOPIC.to_string(), %num_retries, %max_retry_count, local_addr=?self.config.listening_addrs, "subscribed to topic successfully");
                    break;
                }
                Ok(Err(err)) => {
                    error!(topic=%TOPIC.to_string(), %err, %num_retries, %max_retry_count, local_addr=?self.config.listening_addrs, "failed to subscribe to topic, retrying...");
                }
                Err(_) => {
                    error!(topic=%TOPIC.to_string(), %num_retries, %max_retry_count, local_addr=?self.config.listening_addrs, "failed to subscribe to topic, retrying...");
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

        let subscriptions = 0;
        let start_time = Instant::now();
        let mut next_check = Instant::now();

        let connection_future = async {
            while let Some(event) = self.swarm.next().await {
                debug!("received event from swarm");
                trace!(?event, "received event from swarm");

                match event {
                    SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(
                        GossipsubEvent::Subscribed { peer_id, .. },
                    )) => {
                        debug!(%peer_id, %subscriptions, "got subscription from a peer");
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
    pub async fn listen(mut self) {
        loop {
            // Prepare futures for each branch; disabled features use a pending future
            let cancel_fut = self.cancellation_token.cancelled();
            let swarm_fut = self.swarm.select_next_some();
            let cmd_fut = self.commands.recv();

            #[cfg(feature = "gossipsub")]
            let gossip_fut = self.gossip_commands.recv();
            #[cfg(not(feature = "gossipsub"))]
            let gossip_fut = pending::<Option<GossipCommand>>();

            #[cfg(feature = "request-response")]
            let reqresp_fut = self.request_response_commands.recv();
            #[cfg(not(feature = "request-response"))]
            let reqresp_fut = pending::<Option<RequestResponseCommand>>();

            // Select over all branches; disabled futures never complete
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
                    self.handle_gossip_command(gossip_cmd).await
                }
                Some(req_cmd) = reqresp_fut => {
                    self.handle_request_response_command(req_cmd).await
                }
            };

            if let Err(err) = result {
                error!(%err, "Stopping `P2P.listen` : encountered error after which we think we can't continue operating.");
                return;
            }
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
        use crate::swarm::dto::dht_record::SignedRecord;

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
                    match opt_record {
                        Some(ref record_data) => {
                            let res_our_record_data: Result<SignedRecord, serde_json::Error> =
                                serde_json::from_slice(&record_data.value);
                            match res_our_record_data {
                                Ok(_signed_record) => {
                                    let res = self
                                        .swarm
                                        .behaviour_mut()
                                        .kademlia
                                        .store_mut()
                                        .put(opt_record.unwrap());
                                    if res.is_err() {
                                        info!(err = %res.unwrap_err(),"Someone has asked as to put a record that is too big. Refusing to do so.")
                                    }
                                }
                                Err(e) => {
                                    info!(
                                        ?e,
                                        "Someone asked us to put either not valid or not properly signed record. Refusing to keep it."
                                    )
                                }
                            }
                        }
                        None => {
                            info!("Someone asked us to put empty record. Refusing to keep it.");
                        }
                    }
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
                    if self.kademlia_postponed_get_action.contains_key(&id) {
                        let res_record: Result<SignedRecord, serde_json::Error> =
                            serde_json::from_slice(&peer_record.record.value);
                        trace!(
                            ?res_record,
                            "Deserialized and validated. Still can be optional."
                        );
                        match self.kademlia_postponed_get_action.remove(&id).unwrap() {
                            ActionOnKademliaGetRecord::JustThrowResultBack { tx } => {
                                let _ = tx.send(res_record.ok());
                            }
                        };
                    }
                    Ok(())
                }
                QueryResult::GetRecord(Ok(
                    libp2p::kad::GetRecordOk::FinishedWithNoAdditionalRecord { cache_candidates },
                )) => {
                    trace!(
                        %id, ?stats, ?step, ?cache_candidates, "QueryResult::GetRecord(Ok(libp2p::kad::GetRecordOk::FinishedWithNoAdditionalRecord"
                    );
                    if self.kademlia_postponed_get_action.contains_key(&id) {
                        match self.kademlia_postponed_get_action.remove(&id).unwrap() {
                            ActionOnKademliaGetRecord::JustThrowResultBack { tx } => {
                                let _ = tx.send(None);
                            }
                        };
                    }
                    Ok(())
                }
                QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::Timeout { key })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, "QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::Timeout"
                    );
                    if self.kademlia_postponed_get_action.contains_key(&id) {
                        match self.kademlia_postponed_get_action.remove(&id).unwrap() {
                            ActionOnKademliaGetRecord::JustThrowResultBack { tx } => {
                                let _ = tx.send(None);
                            }
                        };
                    }
                    Ok(())
                }
                QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::NotFound {
                    key,
                    closest_peers,
                })) => {
                    trace!(
                        %id, ?stats, ?step, ?key, ?closest_peers, "QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::NotFound"
                    );
                    if self.kademlia_postponed_get_action.contains_key(&id) {
                        match self.kademlia_postponed_get_action.remove(&id).unwrap() {
                            ActionOnKademliaGetRecord::JustThrowResultBack { tx } => {
                                let _ = tx.send(None);
                            }
                        };
                    }
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
                    if self.kademlia_postponed_get_action.contains_key(&id) {
                        match self.kademlia_postponed_get_action.remove(&id).unwrap() {
                            ActionOnKademliaGetRecord::JustThrowResultBack { tx } => {
                                let _ = tx.send(None);
                            }
                        };
                    }
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
                    // since we are manually adding peers, here's a filtering could be implemented
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
                trace!(
                    res = %(!self.kademlia_is_initial_record_already_posted
                                    && self
                                        .swarm
                                        .connected_peers().count()
                                        > self.config.kademlia_threshold),
                    is_already_posted = %self.kademlia_is_initial_record_already_posted,
                    how_many_connections = %self
                                        .swarm
                                        .connected_peers().count(),
                    threshold = %self.config.kademlia_threshold,
                    "Routing updated..."
                );
                // TODO(Arniiiii): get amount not connected peers but how many nodes we know in
                // kademlia
                if !self.kademlia_is_initial_record_already_posted
                    && self.swarm.connected_peers().count() > self.config.kademlia_threshold
                {
                    let maybe_signed_record_data = SignedRecord::new(
                        RecordData::new(
                            self.config.app_public_key.clone(),
                            *self.swarm.local_peer_id(),
                            self.swarm.external_addresses().cloned().collect::<Vec<_>>(),
                        ),
                        &self.signer,
                    );

                    match maybe_signed_record_data {
                        Err(e) => {
                            warn!(%e,
                                app_pk = ?self.config.app_public_key,
                                local_tid = %self.swarm.local_peer_id(),
                                external_addresses = ?self.swarm.external_addresses().cloned().collect::<Vec<_>>(),
                                "Failed to serialize our signed record.");
                        }
                        Ok(signed_record_data) => {
                            let _ = self.swarm.behaviour_mut().kademlia.put_record(
                                Record::new(
                                    self.config.app_public_key.encode_protobuf(),
                                    serde_json::to_vec(&signed_record_data).unwrap(),
                                ),
                                Quorum::Majority,
                            );
                        }
                    };
                    self.kademlia_is_initial_record_already_posted = true;
                }
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
        trace!("Got message: {:?}", &message.data);

        let _ = message
            .source
            .expect("Message must have author as ValidationMode set to Permissive");

        let event = GossipEvent::ReceivedMessage(message.data);

        let propagation_result = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .report_message_validation_result(
                &message_id,
                &propagation_source,
                MessageAcceptance::Accept,
            );

        if !propagation_result {
            warn!(?event, "failed to propagate accepted message further");
        }

        self.gossip_events
            .send(event)
            .map_err(|e| ProtocolError::GossipEventsChannelClosed(e.into()))?;

        Ok(())
    }

    async fn queue_connections(
        &mut self,
        target_app_public_key: &PublicKey,
        mut addresses: Vec<Multiaddr>,
    ) {
        if addresses.is_empty() {
            warn!("No addresses provided to dial");
        }

        if self.dial_manager.has_app_public_key(target_app_public_key) {
            error!(
                "Already dialing peer with app_public_key: {:?}",
                target_app_public_key
            );
        }

        addresses.sort_by_key(|addr| !addr.protocol_stack().any(|proto| proto.contains("quic")));

        let mut queue = VecDeque::from(addresses);

        let first_addr = queue.pop_front().unwrap(); // can use unwrap() here thus we have at least one element

        self.config.connect_to.push(first_addr.clone());
        self.dial_manager
            .insert_queue(target_app_public_key.clone(), queue);
        self.dial_and_map(first_addr, target_app_public_key.clone())
            .await;
    }

    /// Handles command sent through channel by P2P implementation user.
    async fn handle_command(&mut self, cmd: Command) -> P2PResult<()> {
        match cmd {
            Command::ConnectToPeer {
                app_public_key,
                addresses,
            } => {
                self.queue_connections(&app_public_key, addresses).await;
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
            #[cfg(feature = "kad")]
            Command::GetDHTRecord {
                app_public_key,
                response_sender,
            } => {
                self.ask_kademlia_get_record(
                    &app_public_key,
                    ActionOnKademliaGetRecord::JustThrowResultBack {
                        tx: response_sender,
                    },
                );
                Ok(())
            }
        }
    }

    #[cfg(feature = "kad")]
    fn ask_kademlia_get_record(&mut self, app_pk: &PublicKey, action: ActionOnKademliaGetRecord) {
        let queryid = self
            .swarm
            .behaviour_mut()
            .kademlia
            .get_record(RecordKey::new(&app_pk.encode_protobuf()));

        self.kademlia_postponed_get_action.insert(queryid, action);

        trace!(?app_pk, "We inserted queryid -> action");
    }

    /// Handles gossip command sent through GossipHandle.
    #[cfg(feature = "gossipsub")]
    async fn handle_gossip_command(&mut self, cmd: GossipCommand) -> P2PResult<()> {
        debug!("Publishing message");
        trace!("Publishing message {:?}", &cmd.data);

        let message_id = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .publish(TOPIC.hash(), cmd.data)
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
        let request_target_peer_id = match self
            .swarm
            .behaviour_mut()
            .setup
            .get_transport_id_by_application_key(&cmd.target_app_public_key)
        {
            Some(tid) => tid,
            None => {
                warn!(
                    "We don't have transport id for the application public key. Failed to send request because we don't know the peer."
                );
                return Ok(());
            }
        };
        debug!(%request_target_peer_id, "Got a request message to send...");
        trace!(?cmd.data, "Got a request message to send...");

        if self.swarm.is_connected(&request_target_peer_id) {
            self.swarm
                .behaviour_mut()
                .request_response
                .send_request(&request_target_peer_id, cmd.data);
            return Ok(());
        }

        Ok(())
    }

    /// Handles [`RequestResponseEvent`] from the swarm.
    #[instrument(skip(self, event))]
    #[cfg(feature = "request-response")]
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
                warn!(%peer, %error, %request_id, "Inbound failure");
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
        _peer_id: PeerId,
        msg: request_response::Message<Vec<u8>, Vec<u8>, Vec<u8>>,
    ) -> P2PResult<()> {
        let reqresp_timeout = self
            .config
            .channel_timeout
            .unwrap_or(DEFAULT_CHANNEL_TIMEOUT);
        match msg {
            request_response::Message::Request {
                request, channel, ..
            } => {
                {
                    trace!("We got a request!!! we need to answer it!");

                    let (tx, rx) = oneshot::channel();

                    let event = ReqRespEvent::ReceivedRequest(request, tx);
                    let send_result =
                        timeout(reqresp_timeout, self.req_resp_events.send(event)).await;
                    match send_result {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            return Err(errors::SwarmError::Protocol(
                                ProtocolError::ReqRespEventChannelClosed(e.into()),
                            ));
                        }
                        Err(_) => {
                            error!("Timeout while sending ReceivedRequest event");
                        }
                    }

                    let resp = timeout(reqresp_timeout, rx).await;
                    let _ = match resp {
                        Ok(Ok(response)) => self
                            .swarm
                            .behaviour_mut()
                            .request_response
                            .send_response(channel, response)
                            .map_err(|_| {
                                error!("Failed to send response: connection dropped or response channel closed");
                            }),
                        Ok(Err(err)) => {
                            error!("Received error in response: {err:?}");
                            Ok(())
                        }
                        Err(_) => {
                            error!("Timeout waiting for response to request");
                            Ok(())
                        }
                    };
                }
                Ok(())
            }

            request_response::Message::Response { response, .. } => {
                {
                    let event = ReqRespEvent::ReceivedResponse(response);
                    let send_result =
                        timeout(reqresp_timeout, self.req_resp_events.send(event)).await;
                    match send_result {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            return Err(errors::SwarmError::Protocol(
                                ProtocolError::ReqRespEventChannelClosed(e.into()),
                            ));
                        }
                        Err(_) => {
                            error!("Timeout while sending ReceivedResponse event");
                        }
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
                    $signer.clone(),
                    #[cfg(feature = "kad")]
                    &$cfg.kad_protocol_name,
                )
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
                signer,
                #[cfg(feature = "kad")]
                &config.kad_protocol_name,
            )
        })
        .map_err(|e| ProtocolError::BehaviourInitialization(e.into()))?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();
    #[cfg(not(feature = "quic"))]
    let swarm = finish_swarm!(
        builder.with_tcp(
            Default::default(),
            noise::Config::new,
            yamux::Config::default,
        ),
        config,
        signer
    );
    Ok(swarm)
}
