use std::{collections::HashSet, fmt::Debug, io, sync::LazyLock, time::Duration};

use behavior::{Behaviour, BehaviourEvent};
use bitcoin::hashes::Hash;
use futures::StreamExt as _;
use handle::P2PHandle;
use itertools::iproduct;
use libp2p::{
    core::{muxing::StreamMuxerBox, transport::MemoryTransport, ConnectedPoint},
    gossipsub::{Event as GossipsubEvent, Message, MessageAcceptance, MessageId, Sha256Topic},
    identity::secp256k1::Keypair,
    noise, request_response,
    request_response::Event as RequestResponseEvent,
    swarm::SwarmEvent,
    yamux, Multiaddr, PeerId, Swarm, SwarmBuilder, Transport, TransportError,
};
use prost::Message as ProtoMsg;
use strata_p2p_db::{
    DBResult, DepositSetupEntry, GenesisInfoEntry, NoncesEntry, PartialSignaturesEntry,
    RepositoryError, RepositoryExt, Wots160KeysEntry, Wots256KeysEntry, Wots32KeysEntry,
};
use strata_p2p_types::OperatorPubKey;
use strata_p2p_wire::p2p::{
    v1,
    v1::{
        proto,
        proto::{gossipsub_msg::Body, GetMessageRequest, GetMessageResponse},
        GossipsubMsg,
    },
};
use thiserror::Error;
use tokio::{
    select,
    sync::{broadcast, mpsc},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument};

use crate::{commands::Command, events::Event};

mod behavior;
mod codec;
pub mod handle;

// TODO(Velnbur): make this configurable later
/// Global topic name for gossipsub messages.
static TOPIC: LazyLock<Sha256Topic> = LazyLock::new(|| Sha256Topic::new("bitvm2"));

// TODO(Velnbur): make this configurable later
/// Global name of the protocol
const PROTOCOL_NAME: &str = "/strata-bitvm2";

#[derive(Debug, Error)]
pub enum Error {
    #[error("Database error")]
    Repository(#[from] RepositoryError),
    #[error("Validation error")]
    Validation(#[from] ValidationError),
    #[error("Protocol error")]
    Protocol(#[from] ProtocolError),
}

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Invalid signature")]
    InvalidSignature,
    #[error("Not in signers allowlist")]
    NotInSignersAllowlist,
}

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Failed to listen: {0}")]
    Listen(#[from] TransportError<io::Error>),
    #[error("Events channel closed: {0}")]
    EventsChannelClosed(Box<dyn std::error::Error + Sync + Send>),
    #[error("Failed to initialize transport: {0}")]
    TransportInitialization(Box<dyn std::error::Error + Sync + Send>),
    #[error("Failed to initialize behaviour: {0}")]
    BehaviourInitialization(Box<dyn std::error::Error + Sync + Send>),
}

pub type P2PResult<T> = Result<T, Error>;

/// Configuration options for [`P2P`].
pub struct P2PConfig {
    /// Pair of keys used as PeerId.
    pub keypair: Keypair,

    /// Idle connection timeout.
    pub idle_connection_timeout: Duration,

    pub listening_addr: Multiaddr,

    /// List of peers node is allowed to connect to.
    pub allowlist: Vec<PeerId>,

    /// Initial list of nodes to connect to at startup.
    pub connect_to: Vec<Multiaddr>,

    /// List of signers' public keys, whose messages node accepts.
    pub signers_allowlist: Vec<OperatorPubKey>,
}

/// Implementation of p2p protocol for BitVM2 data exchange.
pub struct P2P<DepositSetupPayload: ProtoMsg + Clone, Repository> {
    swarm: Swarm<Behaviour>,

    events: broadcast::Sender<Event<DepositSetupPayload>>,
    commands: mpsc::Receiver<Command<DepositSetupPayload>>,

    // We need this one to create new handles, as only sender is clonable.
    commands_sender: mpsc::Sender<Command<DepositSetupPayload>>,

    db: Repository,

    cancellation_token: CancellationToken,
    config: P2PConfig,
}

/// Alias for P2P and P2PHandle tuple returned by `from_config`.
pub type P2PWithHandle<DSP, Repository> = (P2P<DSP, Repository>, P2PHandle<DSP>);

impl<DSP, DB: RepositoryExt<DSP>> P2P<DSP, DB>
where
    DSP: ProtoMsg + Clone + Default + 'static,
{
    pub fn from_config(
        cfg: P2PConfig,
        cancel: CancellationToken,
        db: DB,
        mut swarm: Swarm<Behaviour>,
    ) -> P2PResult<P2PWithHandle<DSP, DB>> {
        swarm
            .listen_on(cfg.listening_addr.clone())
            .map_err(ProtocolError::Listen)?;

        // TODO(Velnbur): make this configurable
        let (events_tx, events_rx) = broadcast::channel(50_000);
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
            P2PHandle::new(events_rx, cmds_tx),
        ))
    }

    pub fn local_peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }

    /// Create and return a new subscribed handler.
    pub fn new_handle(&self) -> P2PHandle<DSP> {
        P2PHandle::new(self.events.subscribe(), self.commands_sender.clone())
    }

    /// Wait until all connections are established and all peers are subscribed to
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
                    debug!(%address, "Establshed connection with peer");
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

    /// Start listening and handling events from the network and commands from
    /// handles.
    ///
    /// This method should be spawned in separate async task or polled periodicly
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

    async fn handle_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) -> P2PResult<()> {
        match event {
            SwarmEvent::Behaviour(event) => self.handle_behaviour_event(event).await,
            _ => Ok(()),
        }
    }

    async fn handle_behaviour_event(&mut self, event: BehaviourEvent) -> P2PResult<()> {
        match event {
            BehaviourEvent::Gossipsub(event) => self.handle_gossip_event(event).await,
            BehaviourEvent::RequestResponse(event) => {
                self.handle_request_response_event(event).await
            }
            _ => Ok(()),
        }
    }

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

    /// Handle new message from gossipsub network.
    ///
    /// If message is not [`GossipsubMsg`] or is not signed, the message will
    /// be rejected without propagation, otherwise if we didn't handled it
    /// before, send an [`Event`] to handles, store it and reset timeout.
    #[instrument(skip(self, message), fields(sender = %message.source.unwrap()))]
    async fn handle_gossip_msg(
        &mut self,
        propagation_source: PeerId,
        message_id: MessageId,
        message: Message,
    ) -> P2PResult<()> {
        let msg = match GossipsubMsg::<DSP>::from_bytes(&message.data) {
            Ok(msg) => msg,
            Err(err) => {
                debug!(%err, "Got invalid message from peer, rejecting it.");
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
            debug!(reason=%err, "Message failed validation.");
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
                |err| debug!(%err, ?event, "failed to propagate accepted message further"),
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

    /// Add data received from external source to DB if it wasn't there before.
    ///
    /// Returns if the data was already presented or not.
    async fn add_msg_if_not_exists(&mut self, msg: &GossipsubMsg<DSP>) -> DBResult<bool> {
        match &msg.unsigned {
            v1::UnsignedGossipsubMsg::GenesisInfo(info) => {
                self.db
                    .set_genesis_info_if_not_exists(GenesisInfoEntry {
                        entry: (info.pre_stake_outpoint, info.checkpoint_pubkeys.clone()),
                        signature: msg.signature.clone(),
                        key: msg.key.clone(),
                    })
                    .await
            }
            v1::UnsignedGossipsubMsg::DepositSetup { scope, setup } => {
                self.db
                    .set_deposit_setup_if_not_exists(
                        *scope,
                        DepositSetupEntry {
                            payload: setup.payload.clone(),
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
            v1::UnsignedGossipsubMsg::Wots32KeysExchange {
                session_id,
                wots_id,
                keys,
            } => {
                self.db
                    .set_wots32_keys_if_not_exist(
                        *session_id,
                        *wots_id,
                        Wots32KeysEntry {
                            entry: keys.clone(),
                            signature: msg.signature.clone(),
                            key: msg.key.clone(),
                        },
                    )
                    .await
            }
            v1::UnsignedGossipsubMsg::Wots160KeysExchange {
                session_id,
                wots_id,
                keys,
            } => {
                self.db
                    .set_wots160_keys_if_not_exist(
                        *session_id,
                        *wots_id,
                        Wots160KeysEntry {
                            entry: keys.clone(),
                            signature: msg.signature.clone(),
                            key: msg.key.clone(),
                        },
                    )
                    .await
            }
            v1::UnsignedGossipsubMsg::Wots256KeysExchange {
                session_id,
                wots_id,
                keys,
            } => {
                self.db
                    .set_wots256_keys_if_not_exist(
                        *session_id,
                        *wots_id,
                        Wots256KeysEntry {
                            entry: keys.clone(),
                            signature: msg.signature.clone(),
                            key: msg.key.clone(),
                        },
                    )
                    .await
            }
        }
    }

    /// Handle command sent through channel by P2P implementation user.
    async fn handle_command(&mut self, cmd: Command<DSP>) -> P2PResult<()> {
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
                    .inspect_err(|err| debug!(%err, "Failed to publish msg through gossipsub"));

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
        }
    }

    #[instrument(skip(self, event))]
    async fn handle_request_response_event(
        &mut self,
        event: RequestResponseEvent<GetMessageRequest, GetMessageResponse>,
    ) -> P2PResult<()> {
        match event {
            RequestResponseEvent::Message { peer, message } => {
                self.handle_message_event(peer, message).await?
            }
            RequestResponseEvent::OutboundFailure {
                peer,
                request_id,
                error,
            } => {
                debug!(%peer, %error, %request_id, "Outbound failure")
            }
            RequestResponseEvent::InboundFailure {
                peer,
                request_id,
                error,
            } => {
                debug!(%peer, %error, %request_id, "Inbound failure")
            }
            RequestResponseEvent::ResponseSent { peer, request_id } => {
                debug!(%peer, %request_id, "Response sent")
            }
        }

        Ok(())
    }

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
                    debug!(%peer_id, "Peer sent invalid get message request, disconnecting it");
                    let _ = self.swarm.disconnect_peer_id(peer_id);
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .request_response
                        .send_response(channel, empty_response)
                        .inspect_err(|_| debug!("Failed to send response"));

                    return Ok(());
                };

                let Some(msg) = self.handle_get_message_request(req).await? else {
                    debug!(%request_id, "Have no needed data");
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .request_response
                        .send_response(channel, empty_response)
                        .inspect_err(|_| debug!("Failed to send response"));

                    return Ok(());
                };

                let response = GetMessageResponse { msg: vec![msg] };

                let _ = self
                    .swarm
                    .behaviour_mut()
                    .request_response
                    .send_response(channel, response)
                    .inspect_err(|_| debug!("Failed to send response"));
            }

            request_response::Message::Response {
                request_id,
                response,
            } => {
                if response.msg.is_empty() {
                    debug!(%request_id, "Received empty response");
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
                            debug!(%peer_id, reason=%err, "Peer sent invalid message");
                            continue;
                        }
                    };
                    if let Err(err) = self.validate_gossipsub_msg(&msg) {
                        debug!(%peer_id, reason=%err, "Message failed validation");
                        continue;
                    }

                    self.handle_get_message_response(msg).await?
                }
            }
        };

        Ok(())
    }

    async fn handle_get_message_request(
        &mut self,
        request: v1::GetMessageRequest,
    ) -> P2PResult<Option<proto::GossipsubMsg>> {
        let msg = match request {
            v1::GetMessageRequest::Genesis { operator_pk } => {
                let info = self.db.get_genesis_info(&operator_pk).await?;

                info.map(|v| proto::GossipsubMsg {
                    body: Some(Body::GenesisInfo(proto::GenesisInfo {
                        pre_stake_vout: v.entry.0.vout,
                        pre_stake_txid: v.entry.0.txid.to_byte_array().to_vec(),
                        checkpoint_pubkeys: v
                            .entry
                            .1
                            .iter()
                            .map(|k| k.serialize().to_vec())
                            .collect(),
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
                        payload: v.payload.encode_to_vec(),
                    })),
                    signature: v.signature,
                    key: v.key.into(),
                })
            }
            v1::GetMessageRequest::Musig2SignaturesExchange {
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
            v1::GetMessageRequest::Musig2NoncesExchange {
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
            v1::GetMessageRequest::Wots32KeyExchange {
                session_id,
                operator_pk,
                wots_id,
            } => {
                let keys = self
                    .db
                    .get_wots32_keys(&operator_pk, session_id, wots_id)
                    .await?;

                keys.map(|v| proto::GossipsubMsg {
                    body: Some(Body::Wots32Keys(proto::Wots32KeysExchange {
                        session_id: session_id.to_vec(),
                        wots_id,
                        keys: v.entry.iter().map(|k| k.serialize().to_vec()).collect(),
                    })),
                    signature: v.signature,
                    key: v.key.into(),
                })
            }
            v1::GetMessageRequest::Wots160KeyExchange {
                session_id,
                operator_pk,
                wots_id,
            } => {
                let keys = self
                    .db
                    .get_wots160_keys(&operator_pk, session_id, wots_id)
                    .await?;

                keys.map(|v| proto::GossipsubMsg {
                    body: Some(Body::Wots160Keys(proto::Wots160KeysExchange {
                        session_id: session_id.to_vec(),
                        wots_id,
                        keys: v.entry.iter().map(|k| k.serialize().to_vec()).collect(),
                    })),
                    signature: v.signature,
                    key: v.key.into(),
                })
            }
            v1::GetMessageRequest::Wots256KeyExchange {
                session_id,
                operator_pk,
                wots_id,
            } => {
                let keys = self
                    .db
                    .get_wots256_keys(&operator_pk, session_id, wots_id)
                    .await?;

                keys.map(|v| proto::GossipsubMsg {
                    body: Some(Body::Wots256Keys(proto::Wots256KeysExchange {
                        session_id: session_id.to_vec(),
                        wots_id,
                        keys: v.entry.iter().map(|k| k.serialize().to_vec()).collect(),
                    })),
                    signature: v.signature,
                    key: v.key.into(),
                })
            }
        };

        Ok(msg)
    }

    async fn handle_get_message_response(&mut self, msg: GossipsubMsg<DSP>) -> P2PResult<()> {
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
    fn validate_gossipsub_msg(&self, msg: &GossipsubMsg<DSP>) -> P2PResult<()> {
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
/// Macro is used here, as `libp2p` doesn't expose internal generic types
/// of [`SwarmBuilder`] to actually specify return type of function. So we
/// use macro for now.
macro_rules! init_swarm {
    ($cfg:expr) => {
        SwarmBuilder::with_existing_identity($cfg.keypair.clone().into()).with_tokio()
    };
}

/// Finish builder of swarm with parameters from config.
///
/// Again, macro is used as there is no way to specify this behaviour in
/// function, because `with_tcp` and `with_other_transport` return
/// completely different types that can't be generalized.
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

/// Construct swarm from P2P config with inmemory transport. Uses
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

/// Construct swarm from P2P config with TCP transport. Uses
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
