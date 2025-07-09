//! Swarm implementation for P2P.

use std::{
    collections::HashSet,
    sync::LazyLock,
    time::{Duration, Instant},
};

use behavior::{Behaviour, BehaviourEvent};
use errors::{P2PResult, ProtocolError};
use futures::StreamExt as _;
use handle::P2PHandle;
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder, Transport,
    core::{ConnectedPoint, muxing::StreamMuxerBox, transport::MemoryTransport},
    gossipsub::{
        Event as GossipsubEvent, Message, MessageAcceptance, MessageId, PublishError, Sha256Topic,
    },
    identify,
    identity::Keypair,
    kad::{
        AddProviderError, AddProviderOk, BootstrapOk, Event as KademliaEvent, GetClosestPeersError,
        GetClosestPeersOk, GetProvidersError, PutRecordError, PutRecordOk, QueryResult,
    },
    noise,
    request_response::{self, Event as RequestResponseEvent},
    swarm::{
        SwarmEvent,
        dial_opts::{DialOpts, PeerCondition},
    },
    yamux,
};
use tokio::{
    select,
    sync::{broadcast, mpsc},
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, trace, warn};

use crate::{
    commands::{BanUnbanCommand, Command, QueryP2PStateCommand},
    events::Event,
};

mod behavior;
mod codec_raw;
pub mod errors;
pub mod handle;

/// Global topic name for gossipsub messages.
// TODO(Velnbur): make this configurable later
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

/// Configuration options for [`P2P`].
#[derive(Debug, Clone)]
pub struct P2PConfig {
    /// [`Keypair`] used as [`PeerId`].
    pub keypair: Keypair,

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

    /// The node's address.
    pub listening_addr: Multiaddr,

    /// List of [`PeerId`]s that the node is expected to have banned.
    pub blacklist: Vec<PeerId>,

    /// Initial list of nodes to connect to at startup.
    pub connect_to: Vec<Multiaddr>,

    /// Kademlia protocol name
    pub kad_protocol_name: Option<StreamProtocol>,
}

/// Implementation of P2P protocol data exchange.
#[expect(missing_debug_implementations)]
pub struct P2P {
    /// The swarm that handles the networking.
    swarm: Swarm<Behaviour>,

    /// Event channel for the swarm.
    events: broadcast::Sender<Event>,

    /// Command channel for the swarm.
    commands: mpsc::Receiver<Command>,

    /// ([`Clone`]able) Command channel for the swarm.
    ///
    /// # Implementation details
    ///
    /// This is needed because we can't create new handles from the receiver, as
    /// only sender is [`Clone`]able.
    commands_sender: mpsc::Sender<Command>,

    /// Cancellation token for the swarm.
    cancellation_token: CancellationToken,

    /// Underlying configuration.
    config: P2PConfig,
}

/// Alias for P2P and P2PHandle tuple.
pub type P2PWithHandle = (P2P, P2PHandle);

impl P2P {
    /// Creates a new P2P instance from the given configuration.
    pub fn from_config(
        cfg: P2PConfig,
        cancel: CancellationToken,
        mut swarm: Swarm<Behaviour>,
        channel_size: Option<usize>,
    ) -> P2PResult<P2PWithHandle> {
        swarm
            .listen_on(cfg.listening_addr.clone())
            .map_err(ProtocolError::Listen)?;

        let keypair = cfg.keypair.clone();

        let channel_size = channel_size.unwrap_or(256);
        let (events_tx, events_rx) = broadcast::channel(channel_size);
        let (cmds_tx, cmds_rx) = mpsc::channel(64);

        Ok((
            Self {
                swarm,
                events: events_tx,
                commands: cmds_rx,
                commands_sender: cmds_tx.clone(),
                cancellation_token: cancel,
                config: cfg,
            },
            P2PHandle::new(events_rx, cmds_tx, keypair),
        ))
    }

    /// Returns the [`PeerId`] of the local node.
    pub fn local_peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }

    /// Creates a new subscribed handler.
    pub fn new_handle(&self) -> P2PHandle {
        P2PHandle::new(
            self.events.subscribe(),
            self.commands_sender.clone(),
            self.config.keypair.clone(),
        )
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
        let mut counter_how_many_we_dialed_successfully_to = 0_u64;

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
                            counter_how_many_we_dialed_successfully_to += 1;
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

            let mut addr_to_id = Vec::<(Multiaddr, PeerId)>::new();

            while counter_how_many_we_dialed_successfully_to != 0
                && let Some(event) = self.swarm.next().await
            {
                if let SwarmEvent::Behaviour(BehaviourEvent::Identify(
                    identify::Event::Received {
                        connection_id,
                        peer_id,
                        info,
                    },
                )) = event
                {
                    trace!(
                        "{connection_id} {peer_id} {info:?} SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received"
                    );
                    addr_to_id.push((info.listen_addrs[0].clone(), peer_id));
                    counter_how_many_we_dialed_successfully_to -= 1;
                }
            }

            for (multiaddr, peerid) in addr_to_id {
                debug!(%peerid,%multiaddr, %max_retry_count, "adding a peer to Kademlias buckets.");
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peerid, multiaddr);
            }

            debug!("Attempting to bootstrap DHT");
            // let queryid: libp2p::kad::QueryId =
            self.swarm.behaviour_mut().kademlia.bootstrap().unwrap();
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

        let mut subscriptions = 0;
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
                        if self.config.blacklist.contains(&peer_id) {
                            debug!(%peer_id, %subscriptions, total=self.config.blacklist.len(), "got subscription from non-allowlisted peer");
                        } else {
                            subscriptions += 1;
                            info!(%peer_id, %subscriptions, total=self.config.blacklist.len(), "got subscription");
                        }
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
                        total_blacklist=self.config.blacklist.len(),
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
                    total_blacklist=self.config.blacklist.len(),
                    "swarm event loop exited unexpectedly"
                );
            }
            Err(_) => {
                warn!(
                    elapsed=?start_time.elapsed(),
                    remaining_connections=is_not_connected.len(),
                    subscriptions=subscriptions,
                    total_blacklist=self.config.blacklist.len(),
                    "connection establishment timed out after {:?}", general_timeout
                );
            }
        }

        info!("established all connections and subscriptions");
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
            let result = select! {
                _ = self.cancellation_token.cancelled() => {
                    debug!("Received cancellation, stopping listening");
                    return;
                },
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await
                }
                Some(cmd) = self.commands.recv() => {
                    self.handle_command(cmd).await
                },
            };

            if let Err(err) = result {
                error!(%err, "Stopping... encountered error...");
                return;
            }
        }
    }

    /// Handles a [`SwarmEvent`] from the swarm.
    async fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) -> P2PResult<()> {
        match event {
            SwarmEvent::Behaviour(event) => self.handle_behaviour_event(event).await,
            _ => Ok(()),
        }
    }

    /// Handles a [`BehaviourEvent`] from the swarm.
    async fn handle_behaviour_event(&mut self, event: BehaviourEvent) -> P2PResult<()> {
        match event {
            BehaviourEvent::Gossipsub(event) => self.handle_gossip_event(event).await,
            BehaviourEvent::RequestResponse(event) => {
                self.handle_request_response_event(event).await
            }
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
                trace!("{connection_id:?} {peer_id} {info:?} identify::Event::Received");
                self.swarm
                    .behaviour_mut()
                    .kademlia
                    .add_address(&peer_id, info.listen_addrs[0].clone());
            }
            identify::Event::Sent {
                connection_id,
                peer_id,
            } => {
                trace!("{connection_id:?} {peer_id} identify::Event::Sent");
            }
            identify::Event::Pushed {
                connection_id,
                peer_id,
                info,
            } => {
                trace!("{connection_id:?} {peer_id} {info:?} identify::Event::Pushed");
            }
            identify::Event::Error {
                connection_id,
                peer_id,
                error,
            } => {
                trace!("{connection_id:?} {peer_id} {error} identify::Event::Error");
            }
        };
        Ok(())
    }

    /// Handles a [`KademliaEvent`] from the swarm.
    async fn handle_kademlia_event(&mut self, event: KademliaEvent) -> P2PResult<()> {
        match event {
            KademliaEvent::InboundRequest { request } => match request {
                libp2p::kad::InboundRequest::FindNode { num_closer_peers } => {
                    trace!("{num_closer_peers} InboundRequest::FindNode");
                    Ok(())
                }
                libp2p::kad::InboundRequest::PutRecord {
                    source,
                    connection,
                    record,
                } => {
                    trace!(
                        "{} {} {:?} InboundRequest::PutRecord",
                        source, connection, record
                    );
                    Ok(())
                }
                libp2p::kad::InboundRequest::GetRecord {
                    num_closer_peers,
                    present_locally,
                } => {
                    trace!(
                        "
                    {num_closer_peers}
                    {present_locally}

libp2p::kad::InboundRequest::GetRecord
                    "
                    );
                    Ok(())
                }
                libp2p::kad::InboundRequest::GetProvider {
                    num_closer_peers,
                    num_provider_peers,
                } => {
                    trace!(
                        "{num_closer_peers} {num_provider_peers} libp2p::kad::InboundRequest::GetProvider"
                    );
                    Ok(())
                }
                libp2p::kad::InboundRequest::AddProvider { record } => {
                    trace!("{record:?} libp2p::kad::InboundRequest::AddProvider");
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
                        "{id} {stats:?} {step:?} {peer} {num_remaining} QueryResult::Bootstrap(Ok(BootstrapOk "
                    );
                    Ok(())
                }
                QueryResult::Bootstrap(Err(libp2p::kad::BootstrapError::Timeout {
                    peer,
                    num_remaining,
                })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {peer} {num_remaining:?} QueryResult::Bootstrap(Err(BootstrapErr "
                    );
                    Ok(())
                }
                QueryResult::GetRecord(Ok(libp2p::kad::GetRecordOk::FoundRecord(peer_record))) => {
                    trace!(
                        "{id} {stats:?} {step:?} {peer_record:?} QueryResult::GetRecord(Ok(libp2p::kad::GetRecordOk::FoundRecord"
                    );
                    Ok(())
                }
                QueryResult::GetRecord(Ok(
                    libp2p::kad::GetRecordOk::FinishedWithNoAdditionalRecord { cache_candidates },
                )) => {
                    trace!(
                        "{id} {stats:?} {step:?} {cache_candidates:?} QueryResult::GetRecord(Ok(libp2p::kad::GetRecordOk::FinishedWithNoAdditionalRecord"
                    );
                    Ok(())
                }
                QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::Timeout { key })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::Timeout"
                    );
                    Ok(())
                }
                QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::NotFound {
                    key,
                    closest_peers,
                })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} {closest_peers:?} QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::NotFound"
                    );
                    Ok(())
                }
                QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::QuorumFailed {
                    key,
                    records,
                    quorum,
                })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} {records:?} {quorum} QueryResult::GetRecord(Err(libp2p::kad::GetRecordError::QuorumFailed"
                    );
                    Ok(())
                }
                QueryResult::PutRecord(Ok(PutRecordOk { key })) => {
                    trace!("{id} {stats:?} {step:?} {key:?} QueryResult::PutRecord(Ok(PutRecordOk");
                    Ok(())
                }
                QueryResult::PutRecord(Err(PutRecordError::QuorumFailed {
                    key,
                    success: success_peerids,
                    quorum,
                })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} {success_peerids:?} {quorum} QueryResult::PutRecord(Err(PutRecordError::QuorumFailed"
                    );
                    Ok(())
                }
                QueryResult::PutRecord(Err(PutRecordError::Timeout {
                    key,
                    success: success_peerids,
                    quorum,
                })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} {success_peerids:?} {quorum} QueryResult::PutRecord(Err(PutRecordError::Timeout"
                    );
                    Ok(())
                }
                QueryResult::GetProviders(Ok(libp2p::kad::GetProvidersOk::FoundProviders {
                    key,
                    providers,
                })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} {providers:?} QueryResult::GetProviders(Ok(libp2p::kad::GetProvidersOk::FoundProviders"
                    );
                    Ok(())
                }
                QueryResult::GetProviders(Ok(
                    libp2p::kad::GetProvidersOk::FinishedWithNoAdditionalRecord { closest_peers },
                )) => {
                    trace!(
                        "{id} {stats:?} {step:?} {closest_peers:?} QueryResult::GetProviders(Ok(libp2p::kad::GetProvidersOk::FinishedWithNoAdditionalRecord"
                    );
                    Ok(())
                }
                QueryResult::GetProviders(Err(GetProvidersError::Timeout {
                    key,
                    closest_peers,
                })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} {closest_peers:?} QueryResult::GetProviders(Err(GetProvidersError::Timeout"
                    );
                    Ok(())
                }
                QueryResult::GetClosestPeers(Ok(GetClosestPeersOk { key, peers })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} {peers:?} QueryResult::GetClosestPeers(Ok(GetClosestPeersOk"
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
                        "{id} {stats:?} {step:?} {key:?} {peers:?} QueryResult::GetClosestPeers(Ok(GetClosestPeersOk"
                    );
                    Ok(())
                }
                QueryResult::StartProviding(Ok(AddProviderOk { key })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} QueryResult::StartProviding(Ok(AddProviderOk"
                    );
                    Ok(())
                }
                QueryResult::StartProviding(Err(AddProviderError::Timeout { key })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} QueryResult::StartProviding(Err(AddProviderError::Timeout"
                    );
                    Ok(())
                }
                QueryResult::RepublishRecord(Ok(PutRecordOk { key })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} QueryResult::RepublishRecord(Ok(PutRecordOk"
                    );
                    Ok(())
                }
                QueryResult::RepublishRecord(Err(PutRecordError::QuorumFailed {
                    key,
                    success,
                    quorum,
                })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} {success:?} {quorum:?} QueryResult::RepublishRecord(Err(PutRecordError::QuorumFailed"
                    );
                    Ok(())
                }
                QueryResult::RepublishRecord(Err(PutRecordError::Timeout {
                    key,
                    success,
                    quorum,
                })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} {success:?} {quorum} QueryResult::RepublishRecord(Err(PutRecordError::QuorumFailed"
                    );
                    Ok(())
                }
                QueryResult::RepublishProvider(Ok(AddProviderOk { key })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} QueryResult::RepublishProvider(Ok(AddProviderOk"
                    );
                    Ok(())
                }
                QueryResult::RepublishProvider(Err(AddProviderError::Timeout { key })) => {
                    trace!(
                        "{id} {stats:?} {step:?} {key:?} QueryResult::RepublishProvider(Err(AddProviderError::Timeout"
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
                    "{peer:?} {is_new_peer:?} {addresses:?} {bucket_range:?} {old_peer:?} KademliaEvent::RoutingUpdated"
                );
                Ok(())
            }
            KademliaEvent::RoutablePeer { peer, address } => {
                trace!("{peer:?} {address:?} KademliaEvent::RoutablePeer");
                Ok(())
            }
            KademliaEvent::UnroutablePeer { peer } => {
                trace!("{peer:?} KademliaEvent::UnroutablePeer");
                Ok(())
            }

            KademliaEvent::PendingRoutablePeer { peer, address } => {
                trace!("{peer:?} {address:?} KademliaEvent::PendingRoutablePeer");
                Ok(())
            }
            KademliaEvent::ModeChanged { new_mode } => {
                trace!("{new_mode:?} KademliaEvent::ModeChanged");
                Ok(())
            }
        }
    }

    /// Handles a [`GossipsubEvent`] from the swarm.
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

        let event = Event::ReceivedMessage(message.data);

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

        self.events
            .send(event)
            .map_err(|e| ProtocolError::EventsChannelClosed(e.into()))?;

        Ok(())
    }

    /// Handles command sent through channel by P2P implementation user.
    async fn handle_command(&mut self, cmd: Command) -> P2PResult<()> {
        match cmd {
            Command::PublishMessage { data } => {
                debug!("Publishing message");
                trace!("Publishing message {:?}", &data);

                let message_id = self
                    .swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(TOPIC.hash(), data)
                    .inspect_err(|err| {
                        match err {
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
                        }
                    });

                if message_id.is_ok() {
                    debug!(message_id=%message_id.unwrap(), "Message published");
                }

                Ok(())
            }
            Command::RequestMessage { peer_id, data } => {
                let request_target_peer_id = &peer_id;
                debug!(%request_target_peer_id, "Got request message");
                trace!(?data, "Got request message");

                if self.swarm.is_connected(request_target_peer_id) {
                    self.swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(request_target_peer_id, data);
                    return Ok(());
                }

                // TODO(Arniiiii) : rewrite this part so it sends to gossipsub instead of the
                // manual floodsub via manual request-response to everyone.
                let connected_peers = self.swarm.connected_peers().cloned().collect::<Vec<_>>();
                for peer in connected_peers {
                    self.swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(&peer, data.clone());
                }

                Ok(())
            }
            Command::ConnectToPeer(connect_to_peer_command) => {
                // Whitelist peer
                trace!(
                    "Unblock peer: remove from blacklist_behaviour {}",
                    connect_to_peer_command.peer_id
                );
                self.swarm
                    .behaviour_mut()
                    .blacklist_behaviour
                    .unblock_peer(connect_to_peer_command.peer_id);

                // Add the peer to our config lists.
                trace!(
                    "Unblock peer: remove from our lists {}",
                    connect_to_peer_command.peer_id
                );
                self.config
                    .blacklist
                    .retain(|&x| x != connect_to_peer_command.peer_id);

                trace!(
                    "Add peer to connect_to list {}",
                    connect_to_peer_command.peer_addr
                );
                self.config
                    .connect_to
                    .push(connect_to_peer_command.peer_addr.clone());

                // Add peer to swarm
                trace!(
                    "Ask swarm to tell other behaviours that there's a new peer {} {}",
                    connect_to_peer_command.peer_id, connect_to_peer_command.peer_addr
                );
                self.swarm.add_peer_address(
                    connect_to_peer_command.peer_id,
                    connect_to_peer_command.peer_addr.clone(),
                );

                trace!(
                    "Ask explicitly kademlia behaviour to add a new peer {} {}",
                    connect_to_peer_command.peer_id, connect_to_peer_command.peer_addr
                );
                self.swarm.behaviour_mut().kademlia.add_address(
                    &connect_to_peer_command.peer_id,
                    connect_to_peer_command.peer_addr.clone(),
                );

                let dialing_opts: DialOpts = DialOpts::peer_id(connect_to_peer_command.peer_id)
                    .condition(PeerCondition::DisconnectedAndNotDialing)
                    .addresses(Vec::<Multiaddr>::from([connect_to_peer_command
                        .peer_addr
                        .clone()]))
                    .extend_addresses_through_behaviour()
                    .build();

                // Dial peer ( not fully connect )
                trace!(
                    "Try to dial a new peer {} {}",
                    connect_to_peer_command.peer_id, connect_to_peer_command.peer_addr
                );
                let _ = self.swarm.dial(dialing_opts).inspect_err(|err| {
                    error!(
                        "Failed to connect to peer at peer_addr '{}' : {}",
                        connect_to_peer_command.peer_addr.to_string(),
                        err
                    )
                });

                // Bootstrap DHT so that we connect in the end, maybe.
                trace!(
                    "Ask kademlia behaviour to bootstrap itself so that it maybe checks and connects to a new node {} {}",
                    connect_to_peer_command.peer_id, connect_to_peer_command.peer_addr
                );
                let _queryid: libp2p::kad::QueryId =
                    self.swarm.behaviour_mut().kademlia.bootstrap().unwrap();

                // trace!(
                //     "TODO: change waiting for 5 secs to waiting for some events to be triggered
                // {} {}",     connect_to_peer_command.peer_id,
                // connect_to_peer_command.peer_addr );
                // tokio::time::sleep(Duration::from_secs(5)).await;

                Ok(())
            }
            Command::QueryP2PState(query) => match query {
                QueryP2PStateCommand::IsConnected {
                    peer_id,
                    response_sender,
                } => {
                    info!(%peer_id, "Querying if peer is connected");
                    let is_connected = self.swarm.is_connected(&peer_id);
                    let _ = response_sender.send(is_connected);
                    Ok(())
                }
                QueryP2PStateCommand::GetConnectedPeers { response_sender } => {
                    info!("Querying connected peers");
                    let peers = self.swarm.connected_peers().cloned().collect();
                    trace!("Querying connected peers: done: {peers:?}");
                    let _ = response_sender.send(peers);
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
            Command::BanUnbanCommand {
                peer_id,
                peer_addr: _peer_addr,
                ban_unban,
            } => {
                match ban_unban {
                    BanUnbanCommand::Ban => {
                        self.swarm
                            .behaviour_mut()
                            .gossipsub
                            .set_application_score(&peer_id, -9000.0); // TODO: fix it so that it
                        // works with scoring system
                        self.swarm
                            .behaviour_mut()
                            .blacklist_behaviour
                            .block_peer(peer_id);
                    }
                    BanUnbanCommand::Unban => {
                        self.swarm
                            .behaviour_mut()
                            .gossipsub
                            .set_application_score(&peer_id, 9000.0); // TODO: fix it so that it
                        // works with scoring system
                        self.swarm
                            .behaviour_mut()
                            .blacklist_behaviour
                            .unblock_peer(peer_id);
                    }
                }
                Ok(())
            }
        }
    }

    /// Handles [`RequestResponseEvent`] from the swarm.
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
                ..
            } => {
                warn!(%peer, %error, %request_id, "Inbound failure");
                // retry mechanism
                // get the addr from the peer
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
    async fn handle_message_event(
        &mut self,
        _peer_id: PeerId,
        msg: request_response::Message<Vec<u8>, Vec<u8>, Vec<u8>>,
    ) -> P2PResult<()> {
        match msg {
            request_response::Message::Request {
                request,
                channel: _channel,
                ..
            } => {
                let event = Event::ReceivedRequest(request);
                let _ = self
                    .events
                    .send(event)
                    .map_err(|e| ProtocolError::EventsChannelClosed(e.into()))?;

                Ok(())
            }

            request_response::Message::Response {
                request_id,
                response,
            } => {
                if response.is_empty() {
                    warn!(%request_id, ?response, "Received empty response");
                    return Ok(());
                }

                // TODO: report/punish peer for invalid message?
                let event = Event::ReceivedMessage(response);
                let _ = self
                    .events
                    .send(event)
                    .map_err(|e| ProtocolError::EventsChannelClosed(e.into()))?;
                Ok(())
            }
        }
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
        SwarmBuilder::with_existing_identity($cfg.keypair.clone().into()).with_tokio()
    };
}

/// Finishes builder of swarm with parameters from config.
///
/// # Implementation details
///
/// Macro is used as there is no way to specify this behaviour in function, because `with_tcp` and
/// `with_other_transport` return completely different types that can't be generalized.
macro_rules! finish_swarm {
    ($builder:expr, $cfg:expr) => {
        $builder
            .map_err(|e| ProtocolError::TransportInitialization(e.into()))?
            .with_behaviour(|_| {
                Behaviour::new(
                    PROTOCOL_NAME,
                    &$cfg.keypair,
                    &$cfg.blacklist,
                    $cfg.kad_protocol_name
                        .clone()
                        .unwrap_or(StreamProtocol::new("/ipfs/kad_strata-p2p/0.0.1")),
                )
            })
            .map_err(|e| ProtocolError::BehaviourInitialization(e.into()))?
            .with_swarm_config(|c| c.with_idle_connection_timeout($cfg.idle_connection_timeout))
            .build()
    };
}

/// Constructs swarm from P2P config with inmemory transport. Uses
/// `/memory/{n}` addresses.
pub fn with_inmemory_transport(config: &P2PConfig) -> P2PResult<Swarm<Behaviour>> {
    let builder = init_swarm!(config);
    let swarm = finish_swarm!(
        builder.with_other_transport(|our_keypair| {
            MemoryTransport::new()
                .upgrade(libp2p::core::upgrade::Version::V1)
                .authenticate(noise::Config::new(our_keypair).unwrap())
                .multiplex(yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        }),
        config
    );

    Ok(swarm)
}

/// Constructs swarm from P2P config with TCP transport. Uses
/// `/ip4/{addr}/tcp/{port}` addresses.
pub fn with_tcp_transport(config: &P2PConfig) -> P2PResult<Swarm<Behaviour>> {
    let builder = init_swarm!(config);
    let swarm = finish_swarm!(
        builder.with_tcp(
            Default::default(),
            noise::Config::new,
            yamux::Config::default,
        ),
        config
    );

    Ok(swarm)
}
