//! Swarm implementation for P2P.

use std::{collections::HashSet, sync::LazyLock, time::Duration};

use behavior::{Behaviour, BehaviourEvent};
use errors::{P2PResult, ProtocolError, ValidationError};
use futures::StreamExt as _;
use handle::P2PHandle;
use libp2p::{
    core::{muxing::StreamMuxerBox, transport::MemoryTransport, ConnectedPoint},
    gossipsub::{Event as GossipsubEvent, Message, MessageAcceptance, MessageId, Sha256Topic},
    identity::secp256k1::Keypair,
    noise,
    request_response::{self, Event as RequestResponseEvent},
    swarm::SwarmEvent,
    yamux, Multiaddr, PeerId, Swarm, SwarmBuilder, Transport,
};
use prost::Message as ProtoMsg;
use strata_p2p_types::P2POperatorPubKey;
use strata_p2p_wire::p2p::{
    v1,
    v1::{
        GossipsubMsg,
        proto::{GetMessageRequest, GetMessageResponse},
    },
};
use tokio::{
    select,
    sync::{broadcast, mpsc},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument};

use crate::{
    commands::{Command, QueryP2PStateCommand},
    events::Event,
};

mod behavior;
mod codec;
pub mod errors;
pub mod handle;

/// Global topic name for gossipsub messages.
// TODO(Velnbur): make this configurable later
static TOPIC: LazyLock<Sha256Topic> = LazyLock::new(|| Sha256Topic::new("bitvm2"));

/// Global name of the protocol
// TODO(Velnbur): make this configurable later
const PROTOCOL_NAME: &str = "/strata-bitvm2";

/// Configuration options for [`P2P`].
#[derive(Debug, Clone)]
pub struct P2PConfig {
    /// [`Keypair`] used as [`PeerId`].
    pub keypair: Keypair,

    /// Idle connection timeout.
    pub idle_connection_timeout: Duration,

    /// The node's address.
    pub listening_addr: Multiaddr,

    /// List of [`PeerId`]s that the node is allowed to connect to.
    pub allowlist: Vec<PeerId>,

    /// Initial list of nodes to connect to at startup.
    pub connect_to: Vec<Multiaddr>,

    /// List of signers' P2P public keys, whose messages the node is allowed to accept.
    pub signers_allowlist: Vec<P2POperatorPubKey>,
}

/// Implementation of p2p protocol for BitVM2 data exchange.
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

        // WOTS PKs are biiiiiiiig
        let channel_size = channel_size.unwrap_or(400_000);
        let (events_tx, events_rx) = broadcast::channel(channel_size);
        let (cmds_tx, cmds_rx) = mpsc::channel(50_000);

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
        let mut is_not_connected = HashSet::new();
        for addr in &self.config.connect_to {
            // TODO(Velnbur): add retry mechanism later...
            let _ = self
                .swarm
                .dial(addr.clone())
                .inspect_err(|err| error!(%err, %addr, "Failed to dial peer"));
            is_not_connected.insert(addr);
        }

        // TODO(Velnbur): add retry mechanism later...
        let _ = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&TOPIC)
            .inspect_err(|err| error!(%err, "Failed to subscribe for events"));

        let mut subscriptions = 0;

        while let Some(event) = self.swarm.next().await {
            match event {
                SwarmEvent::Behaviour(BehaviourEvent::Gossipsub(GossipsubEvent::Subscribed {
                    peer_id,
                    ..
                })) => {
                    if self.config.allowlist.contains(&peer_id) {
                        subscriptions += 1;
                    }
                    debug!(%peer_id, "Got subscription");
                }
                SwarmEvent::ConnectionEstablished { endpoint, .. } => {
                    let ConnectedPoint::Dialer { address, .. } = endpoint else {
                        continue;
                    };
                    debug!(%address, "Established connection with peer");
                    is_not_connected.remove(&address);
                }
                _ => {}
            };

            if is_not_connected.is_empty() && subscriptions >= self.config.allowlist.len() {
                break;
            }
        }

        info!("Established all connections and subscriptions");
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
        let msg = match GossipsubMsg::from_bytes(&message.data) {
            Ok(msg) => msg,
            Err(err) => {
                error!(%err, "Got invalid message from peer, rejecting it.");
                // no error should appear in case of message rejection
                let _ = self
                    .swarm
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

        let source = message
            .source
            .expect("Message must have author as ValidationMode set to Permissive");
        if let Err(err) = self.validate_gossipsub_msg(&msg) {
            error!(reason=%err, %source, "Message failed validation.");
            // no error should appear in case of message rejection
            let _ = self
                .swarm
                .behaviour_mut()
                .gossipsub
                .report_message_validation_result(
                    &message_id,
                    &propagation_source,
                    MessageAcceptance::Reject,
                );
            return Ok(());
        }

        let event = Event::from(msg);

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
            error!(?event, "failed to propagate accepted message further");
        }

        self.events
            .send(event)
            .map_err(|e| ProtocolError::EventsChannelClosed(e.into()))?;

        Ok(())
    }

    /// Handles command sent through channel by P2P implementation user.
    async fn handle_command(&mut self, cmd: Command) -> P2PResult<()> {
        match cmd {
            Command::PublishMessage(send_message) => {
                let msg: GossipsubMsg = send_message.into();
                debug!(?msg, "Publishing message");

                // TODO(Velnbur): add retry mechanism later, instead of skipping the error
                let _ = self
                    .swarm
                    .behaviour_mut()
                    .gossipsub
                    .publish(TOPIC.hash(), msg.into_raw().encode_to_vec())
                    .inspect_err(|err| error!(%err, "Failed to publish msg through gossipsub"));

                Ok(())
            }
            Command::RequestMessage(request) => {
                let request_target_pubkey = request.operator_pubkey();
                let request_target_peer_id = request.peer_id();
                info!(%request_target_pubkey, %request_target_peer_id, "Got request message");
                debug!(?request, "Got request message");

                let request = request.into_msg();

                if self.swarm.is_connected(&request_target_peer_id) {
                    self.swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(&request_target_peer_id, request);
                    return Ok(());
                }

                let connected_peers = self.swarm.connected_peers().cloned().collect::<Vec<_>>();
                for peer in connected_peers {
                    self.swarm
                        .behaviour_mut()
                        .request_response
                        .send_request(&peer, request.clone());
                }

                Ok(())
            }
            Command::ConnectToPeer(connect_to_peer_command) => {
                // Whitelist peer
                self.config.allowlist.push(connect_to_peer_command.peer_id);
                self.config
                    .connect_to
                    .push(connect_to_peer_command.peer_addr.clone());

                // Add peer to swarm
                self.swarm.add_peer_address(
                    connect_to_peer_command.peer_id,
                    connect_to_peer_command.peer_addr.clone(),
                );

                // Connect to peer
                let _ = self
                    .swarm
                    .dial(connect_to_peer_command.peer_addr)
                    .inspect_err(|err| error!(%err, "Failed to connect to peer"));

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
                    let _ = response_sender.send(peers);
                    Ok(())
                }
            },
        }
    }

    /// Handles [`RequestResponseEvent`] from the swarm.
    #[instrument(skip(self, event))]
    async fn handle_request_response_event(
        &mut self,
        event: RequestResponseEvent<GetMessageRequest, GetMessageResponse>,
    ) -> P2PResult<()> {
        match event {
            RequestResponseEvent::Message { peer, message, .. } => {
                debug!(%peer, ?message, "Received message");
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
                error!(%peer, %error, %request_id, "Inbound failure")
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
        peer_id: PeerId,
        msg: request_response::Message<GetMessageRequest, GetMessageResponse, GetMessageResponse>,
    ) -> P2PResult<()> {
        match msg {
            request_response::Message::Request {
                request,
                channel,
                ..
            } => {
                let empty_response = GetMessageResponse { msg: vec![] };

                let Ok(req) = v1::GetMessageRequest::from_msg(request) else {
                    error!(%peer_id, "Peer sent invalid get message request, replying with empty response");
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .request_response
                        .send_response(channel, empty_response)
                        .inspect_err(|_| error!("Failed to send response"));

                    return Ok(());
                };

                let event = Event::ReceivedRequest(req);
                let _ = self.events.send(event).map_err(|e| ProtocolError::EventsChannelClosed(e.into()))?;

                Ok(())
            }

            request_response::Message::Response {
                request_id,
                response,
            } => {
                if response.msg.is_empty() {
                    error!(%request_id, ?response, "Received empty response");
                    return Ok(());
                }

                for msg in response.msg.into_iter() {
                    if msg.body.is_none() {
                        continue;
                    }

                    // TODO: report/punish peer for invalid message?
                    let msg = match GossipsubMsg::from_proto(msg.clone()) {
                        Ok(msg) => msg,
                        Err(err) => {
                            error!(%peer_id, reason=%err, "Peer sent invalid message");
                            continue;
                        }
                    };
                    if let Err(err) = self.validate_gossipsub_msg(&msg) {
                        error!(%peer_id, reason=%err, "Message failed validation");
                        continue;
                    }

                    let event = Event::ReceivedMessage(msg);
                    let _ = self.events.send(event).map_err(|e| ProtocolError::EventsChannelClosed(e.into()))?;
                }
                Ok(())
            }
        }
    }


    /// Checks gossip sub message for validity by protocol rules.
    fn validate_gossipsub_msg(&self, msg: &GossipsubMsg) -> P2PResult<()> {
        if !self.config.signers_allowlist.contains(&msg.key) {
            return Err(ValidationError::NotInSignersAllowlist.into());
        }

        let content = msg.content();
        if !msg.key.verify(&content, &msg.signature) {
            return Err(ValidationError::InvalidSignature.into());
        }

        Ok(())
    }
}

/// Constructs swarm builder with existing identity.
///
/// # Implementation details
///
/// Macro is used here, as `libp2p` doesn't expose internal generic types of [`SwarmBuilder`] to
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
            .with_behaviour(|_| Behaviour::new(PROTOCOL_NAME, &$cfg.keypair, &$cfg.allowlist))
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
        builder.with_other_transport(|keys| {
            MemoryTransport::new()
                .upgrade(libp2p::core::upgrade::Version::V1)
                .authenticate(noise::Config::new(keys).unwrap())
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
