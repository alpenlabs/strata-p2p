//! Swarm implementation for P2P.

use std::{collections::HashSet, sync::LazyLock, time::Duration};

use behavior::{Behaviour, BehaviourEvent};
use bitcoin::hashes::Hash;
use errors::{P2PResult, ProtocolError, ValidationError};
use futures::StreamExt as _;
use handle::P2PHandle;
use itertools::iproduct;
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
use strata_p2p_db::{
    DBResult, DepositSetupEntry, NoncesEntry, PartialSignaturesEntry, RepositoryExt,
    StakeChainEntry,
};
use strata_p2p_types::P2POperatorPubKey;
use strata_p2p_wire::p2p::{
    v1,
    v1::{
        proto,
        proto::{gossipsub_msg::Body, GetMessageRequest, GetMessageResponse},
        GossipsubMsg,
    },
};
use tokio::{
    select,
    sync::{broadcast, mpsc},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument};

use crate::{commands::Command, events::Event};

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
pub struct P2P<Repository> {
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

    /// The database instance.
    db: Repository,

    /// Cancellation token for the swarm.
    cancellation_token: CancellationToken,

    /// Underlying configuration.
    config: P2PConfig,
}

/// Alias for P2P and P2PHandle tuple.
pub type P2PWithHandle<Repository> = (P2P<Repository>, P2PHandle);

impl<DB: RepositoryExt> P2P<DB> {
    /// Creates a new P2P instance from the given configuration.
    pub fn from_config(
        cfg: P2PConfig,
        cancel: CancellationToken,
        db: DB,
        mut swarm: Swarm<Behaviour>,
        channel_size: Option<usize>,
    ) -> P2PResult<P2PWithHandle<DB>> {
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
                db,
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
            error!(reason=%err, "Message failed validation.");
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

        let new_event = self.add_msg_if_not_exists(&msg).await?;

        // For each message we get, take track of peers from which
        // this message was sent from, so we can request directly from
        // them something, knowing signer's (operator's) node peer id.
        self.db.set_peer_for_signer_pubkey(&msg.key, source).await?;

        let event = Event::from(msg);

        let _ = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .report_message_validation_result(
                &message_id,
                &propagation_source,
                MessageAcceptance::Accept,
            )
            .inspect_err(
                |err| error!(%err, ?event, "failed to propagate accepted message further"),
            );

        // Do not broadcast new event to "handles" if it's not new.
        if !new_event {
            return Ok(());
        }

        self.events
            .send(event)
            .map_err(|e| ProtocolError::EventsChannelClosed(e.into()))?;

        Ok(())
    }

    /// Adds data received from external source to DB if it wasn't there before.
    ///
    /// Returns a `bool` indicating if the data was already presented or not.
    async fn add_msg_if_not_exists(&mut self, msg: &GossipsubMsg) -> DBResult<bool> {
        match &msg.unsigned {
            v1::UnsignedGossipsubMsg::StakeChainExchange {
                stake_chain_id,
                pre_stake_txid,
                pre_stake_vout,
            } => {
                self.db
                    .set_stake_chain_info_if_not_exists(
                        *stake_chain_id,
                        StakeChainEntry {
                            entry: (*pre_stake_txid, *pre_stake_vout),
                            signature: msg.signature.clone(),
                            key: msg.key.clone(),
                        },
                    )
                    .await
            }
            v1::UnsignedGossipsubMsg::DepositSetup {
                scope,
                hash,
                funding_txid,
                funding_vout,
                operator_pk,
                wots_pks,
            } => {
                self.db
                    .set_deposit_setup_if_not_exists(
                        *scope,
                        DepositSetupEntry {
                            hash: *hash,
                            funding_txid: *funding_txid,
                            funding_vout: *funding_vout,
                            operator_pk: *operator_pk,
                            wots_pks: wots_pks.clone(),
                            signature: msg.signature.clone(),
                            key: msg.key.clone(),
                        },
                    )
                    .await
            }
            v1::UnsignedGossipsubMsg::Musig2NoncesExchange { session_id, nonces } => {
                self.db
                    .set_pub_nonces_if_not_exist(
                        *session_id,
                        NoncesEntry {
                            entry: nonces.clone(),
                            signature: msg.signature.clone(),
                            key: msg.key.clone(),
                        },
                    )
                    .await
            }
            v1::UnsignedGossipsubMsg::Musig2SignaturesExchange {
                session_id,
                signatures,
            } => {
                self.db
                    .set_partial_signatures_if_not_exists(
                        *session_id,
                        PartialSignaturesEntry {
                            entry: signatures.clone(),
                            signature: msg.signature.clone(),
                            key: msg.key.clone(),
                        },
                    )
                    .await
            }
        }
    }

    /// Handles command sent through channel by P2P implementation user.
    async fn handle_command(&mut self, cmd: Command) -> P2PResult<()> {
        match cmd {
            Command::PublishMessage(send_message) => {
                let msg = send_message.into();

                self.add_msg_if_not_exists(&msg).await?;

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

                let maybe_distributor = self
                    .db
                    .get_peer_by_signer_pubkey(request_target_pubkey)
                    .await?;

                let request = request.into_msg();

                if let Some(distributor_peer_id) = maybe_distributor {
                    if self.swarm.is_connected(&distributor_peer_id) {
                        self.swarm
                            .behaviour_mut()
                            .request_response
                            .send_request(&distributor_peer_id, request);
                        return Ok(());
                    } // TODO: try to establish connection?
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
            Command::CleanStorage(cmd) => {
                // Get cartesian product of all provided operators and session ids
                let operator_session_id_pairs =
                    iproduct!(&cmd.operators, &cmd.session_ids).collect::<Vec<_>>();

                if !operator_session_id_pairs.is_empty() {
                    self.db
                        .delete_partial_signatures(&operator_session_id_pairs)
                        .await?;
                    self.db
                        .delete_pub_nonces(&operator_session_id_pairs)
                        .await?;
                }

                // Get cartesian product of all provided operators and scopes
                let operator_scope_pairs =
                    iproduct!(&cmd.operators, &cmd.scopes).collect::<Vec<_>>();

                if !operator_scope_pairs.is_empty() {
                    self.db.delete_deposit_setups(&operator_scope_pairs).await?;
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
        }
    }

    /// Handles [`RequestResponseEvent`] from the swarm.
    #[instrument(skip(self, event))]
    async fn handle_request_response_event(
        &mut self,
        event: RequestResponseEvent<GetMessageRequest, GetMessageResponse>,
    ) -> P2PResult<()> {
        match event {
            RequestResponseEvent::Message { peer, message } => {
                debug!(%peer, ?message, "Received message");
                self.handle_message_event(peer, message).await?
            }
            RequestResponseEvent::OutboundFailure {
                peer,
                request_id,
                error,
            } => {
                error!(%peer, %error, %request_id, "Outbound failure")
            }
            RequestResponseEvent::InboundFailure {
                peer,
                request_id,
                error,
            } => {
                error!(%peer, %error, %request_id, "Inbound failure")
            }
            RequestResponseEvent::ResponseSent { peer, request_id } => {
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
                request_id,
                request,
                channel,
            } => {
                let empty_response = GetMessageResponse { msg: vec![] };

                let Ok(req) = v1::GetMessageRequest::from_msg(request) else {
                    error!(%peer_id, "Peer sent invalid get message request, disconnecting it");
                    let _ = self.swarm.disconnect_peer_id(peer_id);
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .request_response
                        .send_response(channel, empty_response)
                        .inspect_err(|_| error!("Failed to send response"));

                    return Ok(());
                };

                let Some(msg) = self.handle_get_message_request(req).await? else {
                    debug!(%request_id, "Have no needed data");
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .request_response
                        .send_response(channel, empty_response)
                        .inspect_err(|_| error!("Failed to send response"));

                    return Ok(());
                };

                let response = GetMessageResponse { msg: vec![msg] };

                let _ = self
                    .swarm
                    .behaviour_mut()
                    .request_response
                    .send_response(channel, response)
                    .inspect_err(|_| error!("Failed to send response"));
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

                    self.handle_get_message_response(msg).await?
                }
            }
        };

        Ok(())
    }

    /// Handles [`v1::GetMessageRequest`] from the swarm.
    async fn handle_get_message_request(
        &mut self,
        request: v1::GetMessageRequest,
    ) -> P2PResult<Option<proto::GossipsubMsg>> {
        let msg = match request {
            v1::GetMessageRequest::StakeChainExchange {
                stake_chain_id,
                operator_pk,
            } => {
                let info = self
                    .db
                    .get_stake_chain_info(&operator_pk, &stake_chain_id)
                    .await?;

                info.map(|v| proto::GossipsubMsg {
                    body: Some(Body::StakeChain(proto::StakeChainExchange {
                        stake_chain_id: stake_chain_id.to_vec(),
                        pre_stake_txid: v.entry.0.to_byte_array().to_vec(),
                        pre_stake_vout: v.entry.1,
                    })),
                    signature: v.signature,
                    key: v.key.into(),
                })
            }
            v1::GetMessageRequest::DepositSetup { scope, operator_pk } => {
                let setup = self.db.get_deposit_setup(&operator_pk, scope).await?;

                setup.map(|v| proto::GossipsubMsg {
                    body: Some(Body::Setup(proto::DepositSetupExchange {
                        scope: scope.to_vec(),
                        hash: v.hash.to_byte_array().to_vec(),
                        funding_txid: v.funding_txid.to_byte_array().to_vec(),
                        funding_vout: v.funding_vout,
                        operator_pk: v.operator_pk.serialize().to_vec(),
                        wots_pks: v.wots_pks.to_flattened_bytes().to_vec(),
                    })),
                    signature: v.signature,
                    key: v.key.into(),
                })
            }
            v1::GetMessageRequest::Musig2SignaturesExchange {
                session_id,
                operator_pk,
            } => {
                let sigs = self
                    .db
                    .get_partial_signatures(&operator_pk, session_id)
                    .await?;

                sigs.map(|v| proto::GossipsubMsg {
                    body: Some(Body::Sigs(proto::Musig2SignaturesExchange {
                        session_id: session_id.to_vec(),
                        partial_sigs: v.entry.iter().map(|n| n.serialize().to_vec()).collect(),
                    })),
                    signature: v.signature,
                    key: v.key.into(),
                })
            }
            v1::GetMessageRequest::Musig2NoncesExchange {
                session_id,
                operator_pk,
            } => {
                let nonces = self.db.get_pub_nonces(&operator_pk, session_id).await?;

                nonces.map(|v| proto::GossipsubMsg {
                    body: Some(Body::Nonce(proto::Musig2NoncesExchange {
                        session_id: session_id.to_vec(),
                        pub_nonces: v.entry.iter().map(|n| n.serialize().to_vec()).collect(),
                    })),
                    signature: v.signature,
                    key: v.key.into(),
                })
            }
        };

        Ok(msg)
    }

    /// Handles [`v1::GetMessageResponse`] from the swarm.
    async fn handle_get_message_response(&mut self, msg: GossipsubMsg) -> P2PResult<()> {
        let new_event = self.add_msg_if_not_exists(&msg).await?;

        if new_event {
            let event = Event::from(msg);

            self.events
                .send(event)
                .map_err(|e| ProtocolError::EventsChannelClosed(e.into()))?;
        }

        Ok(())
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
