//! Swarm implementation for P2P.

use std::{
    collections::HashSet,
    sync::LazyLock,
    time::{Duration, Instant},
};

use behavior::{Behaviour, BehaviourEvent};
use errors::{P2PResult, ProtocolError, SwarmError};
use futures::StreamExt as _;
#[cfg(feature = "request-response")]
use handle::ReqRespHandle;
use handle::{CommandHandle, GossipHandle};
#[cfg(feature = "request-response")]
use libp2p::request_response::{self, Event as RequestResponseEvent};
use libp2p::{
    Multiaddr, PeerId, Swarm, SwarmBuilder, Transport,
    core::{ConnectedPoint, muxing::StreamMuxerBox, transport::MemoryTransport},
    gossipsub::{
        Event as GossipsubEvent, Message, MessageAcceptance, MessageId, PublishError, Sha256Topic,
    },
    identity::{Keypair, PublicKey},
    noise,
    swarm::{
        NetworkBehaviour, SwarmEvent,
        dial_opts::{DialOpts, PeerCondition},
    },
    yamux,
};
#[cfg(feature = "request-response")]
use tokio::sync::oneshot;
use tokio::{
    select,
    sync::{broadcast, mpsc},
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, trace, warn};

#[cfg(feature = "request-response")]
use crate::events::ReqRespEvent;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    events::GossipEvent,
    signer::ApplicationSigner,
    swarm::setup::events::SetupBehaviourEvent,
};

mod behavior;
mod codec_raw;
pub mod errors;
pub mod handle;
mod message;
pub mod setup;

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

/// Global default timeout for channel operations (e.g., sending/receiving on channels).
pub const DEFAULT_CHANNEL_TIMEOUT: Duration = Duration::from_secs(5);

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

    /// The node's address.
    pub listening_addr: Multiaddr,

    /// Initial list of nodes to connect to at startup.
    pub connect_to: Vec<Multiaddr>,

    /// Timeout for channel operations (e.g., sending/receiving on channels).
    #[cfg(feature = "request-response")]
    pub channel_timeout: Option<Duration>,
}

/// Implementation of P2P protocol data exchange.
#[expect(missing_debug_implementations, dead_code)]
pub struct P2P<S: ApplicationSigner> {
    /// The swarm that handles the networking.
    swarm: Swarm<Behaviour<S>>,

    /// Event channel for the gossip.
    gossip_events: broadcast::Sender<GossipEvent>,

    /// Event channel for request/response.
    #[cfg(feature = "request-response")]
    req_resp_events: mpsc::Sender<ReqRespEvent>,

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

    /// Allow list.
    allowlist: HashSet<PublicKey>,

    /// Application signer for signing setup messages.
    signer: S,
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
        swarm
            .listen_on(cfg.listening_addr.clone())
            .map_err(ProtocolError::Listen)?;

        let channel_size = channel_size.unwrap_or(256);
        let (gossip_events_tx, _gossip_events_rx) = broadcast::channel(channel_size);
        let (req_resp_event_tx, req_resp_event_rx) = mpsc::channel(64);
        let (cmds_tx, cmds_rx) = mpsc::channel(64);

        Ok((
            Self {
                swarm,
                gossip_events: gossip_events_tx,
                req_resp_events: req_resp_event_tx,
                commands: cmds_rx,
                commands_sender: cmds_tx.clone(),
                cancellation_token: cancel,
                allowlist: HashSet::from_iter(allowlist),
                config: cfg,
                signer,
            },
            ReqRespHandle::new(req_resp_event_rx),
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
    ) -> P2PResult<P2P<S>> {
        swarm
            .listen_on(cfg.listening_addr.clone())
            .map_err(ProtocolError::Listen)?;

        let channel_size = channel_size.unwrap_or(256);
        let (gossip_events_tx, _gossip_events_rx) = broadcast::channel(channel_size);
        let (cmds_tx, cmds_rx) = mpsc::channel(64);

        Ok(Self {
            swarm,
            gossip_events: gossip_events_tx,
            commands: cmds_rx,
            commands_sender: cmds_tx.clone(),
            cancellation_token: cancel,
            config: cfg,
            allowlist: HashSet::from_iter(allowlist),
            signer,
        })
    }

    /// Returns the [`PeerId`] of the local node.
    pub fn local_peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }

    /// Creates new handle for gossip.
    pub fn new_gossip_handle(&self) -> GossipHandle {
        GossipHandle::new(self.gossip_events.subscribe())
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
    async fn handle_swarm_event(
        &mut self,
        event: SwarmEvent<<Behaviour<S> as NetworkBehaviour>::ToSwarm>,
    ) -> P2PResult<()> {
        match event {
            SwarmEvent::Behaviour(event) => self.handle_behaviour_event(event).await,
            _ => Ok(()),
        }
    }

    /// Handles a [`BehaviourEvent`] from the swarm.
    async fn handle_behaviour_event(
        &mut self,
        event: <Behaviour<S> as NetworkBehaviour>::ToSwarm,
    ) -> P2PResult<()> {
        match event {
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
            Command::RequestMessage {
                app_public_key: app_pk,
                data,
            } => {
                debug!(?app_pk, "Got request message");
                trace!(?data, "Got request message");

                let option_request_target_peer_id = self
                    .swarm
                    .behaviour()
                    .setup
                    .get_transport_id_by_application_key(&app_pk);

                match option_request_target_peer_id {
                    Some(request_target_peer_id) => {
                        if self.swarm.is_connected(&request_target_peer_id) {
                            self.swarm
                                .behaviour_mut()
                                .request_response
                                .send_request(&request_target_peer_id, data);
                            return Ok(());
                        }

                        Ok(())
                    }
                    None => Err(SwarmError::Logic(
                        errors::LogicError::RequestResponseBeforeSetup,
                    )),
                }
            }
            Command::ConnectToPeer(connect_to_peer_command) => {
                // Add peer to swarm
                self.swarm.add_peer_address(
                    connect_to_peer_command.peer_id,
                    connect_to_peer_command.peer_addr.clone(),
                );

                let dialing_opts: DialOpts = DialOpts::peer_id(connect_to_peer_command.peer_id)
                    .condition(PeerCondition::DisconnectedAndNotDialing)
                    .addresses(Vec::<Multiaddr>::from([connect_to_peer_command
                        .peer_addr
                        .clone()]))
                    .extend_addresses_through_behaviour()
                    .build();

                // Connect to peer
                let _ = self.swarm.dial(dialing_opts).inspect_err(|err| {
                    error!(
                        "Failed to connect to peer at peer_addr '{}' : {} {:?}",
                        connect_to_peer_command.peer_addr.to_string(),
                        err,
                        err
                    )
                });

                Ok(())
            }
            Command::QueryP2PState(query) => match query {
                QueryP2PStateCommand::IsConnected {
                    transport_id: peer_id,
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
                QueryP2PStateCommand::GetAppPublicKey {
                    transport_id: peer_id,
                    response_sender,
                } => {
                    let app_pk = self
                        .swarm
                        .behaviour()
                        .setup
                        .get_app_public_key_by_transport_id(&peer_id);
                    info!(%peer_id, ?app_pk, "Querying app public key for peer");
                    let _ = response_sender.send(app_pk);
                    Ok(())
                }
            },
        }
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
                    let (tx, rx) = oneshot::channel();

                    let event = ReqRespEvent::ReceivedRequest(request, tx);
                    let send_result =
                        timeout(reqresp_timeout, self.req_resp_events.send(event)).await;
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
                            return Err(SwarmError::Protocol(
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

/// Constructs swarm from P2P config with TCP transport. Uses
/// `/ip4/{addr}/tcp/{port}` addresses.
pub fn with_tcp_transport<S: ApplicationSigner>(
    config: &P2PConfig,
    signer: S,
) -> P2PResult<Swarm<Behaviour<S>>> {
    let builder = init_swarm!(config);
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
