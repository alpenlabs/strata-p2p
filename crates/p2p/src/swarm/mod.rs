use std::{collections::HashSet, sync::LazyLock, time::Duration};

use behavior::{Behaviour, BehaviourEvent};
use bitcoin::hashes::{sha256, Hash};
use futures::StreamExt as _;
use handle::P2PHandle;
use libp2p::{
    core::{muxing::StreamMuxerBox, transport::MemoryTransport, ConnectedPoint},
    gossipsub::{Event as GossipsubEvent, Message, MessageAcceptance, MessageId, Sha256Topic},
    identity::secp256k1::Keypair,
    noise, request_response,
    request_response::Event as RequestResponseEvent,
    swarm::SwarmEvent,
    yamux, Multiaddr, PeerId, Swarm, SwarmBuilder, Transport,
};
use prost::Message as ProtoMsg;
use snafu::prelude::*;
use strata_p2p_db::{
    states::PeerDepositState, DBResult, DepositSetupEntry, GenesisInfoEntry, NoncesEntry,
    PartialSignaturesEntry, RepositoryError, RepositoryExt,
};
use strata_p2p_wire::p2p::{
    v1,
    v1::{
        proto,
        proto::{
            get_message_request, gossipsub_msg::Body, DepositRequestKey, GetMessageRequest,
            GetMessageResponse,
        },
        DepositNonces, DepositSetup, DepositSigs, GetMessageRequestExchangeKind, GossipsubMsg,
        GossipsubMsgDepositKind, GossipsubMsgKind,
    },
};
use tokio::{
    select,
    sync::{broadcast, mpsc},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument};

use crate::{
    commands::Command,
    events::{Event, EventKind},
    timeouts::{TimeoutEvent, TimeoutsManager},
};

mod behavior;
mod codec;
pub mod handle;

// TODO(Velnbur): make this configurable later
/// Global topic name for gossipsub messages.
static TOPIC: LazyLock<Sha256Topic> = LazyLock::new(|| Sha256Topic::new("bitvm2"));

// TODO(Velnbur): make this configurable later
/// Global name of the protocol
const PROTOCOL_NAME: &str = "/strata-bitvm2";

#[derive(Snafu, Debug)]
pub enum Error {
    #[snafu(display("Database error: {source}"))]
    Repository { source: RepositoryError },

    #[snafu(whatever, display("{message}"))]
    Whatever {
        message: String,
        #[snafu(source(from(Box<dyn std::error::Error>, Some)))]
        source: Option<Box<dyn std::error::Error>>,
    },
}

pub type P2PResult<T> = Result<T, Error>;

/// Configuration options for [`P2P`].
pub struct P2PConfig {
    /// Duration which will be considered stale after last moment current operator received message
    /// that advanced it's state.
    pub next_stage_timeout: Duration,

    /// Pair of keys used as PeerId and for signing.
    pub keypair: Keypair,

    /// Idle connection timeout.
    pub idle_connection_timeout: Duration,

    pub listening_addr: Multiaddr,

    /// List of peers node is allowed to connect to.
    pub allowlist: Vec<PeerId>,

    /// Initial list of nodes to connect to at startup.
    pub connect_to: Vec<Multiaddr>,
}

/// Implementation of p2p protocol for BitVM2 data exchange.
pub struct P2P<DepositSetupPayload: ProtoMsg + Clone, Repository> {
    swarm: Swarm<Behaviour>,

    events: broadcast::Sender<Event<DepositSetupPayload>>,
    commands: mpsc::Receiver<Command<DepositSetupPayload>>,

    // We need this one to create new handles, as only sender is clonable.
    commands_sender: mpsc::Sender<Command<DepositSetupPayload>>,

    db: Repository,
    timeouts_mng: TimeoutsManager,

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
            .whatever_context("failed to listen")?;

        // TODO(Velnbur): make this configurable
        let (events_tx, events_rx) = broadcast::channel(50_000);
        let (cmds_tx, cmds_rx) = mpsc::channel(50_000);
        let timeouts = TimeoutsManager::new();

        Ok((
            Self {
                swarm,
                events: events_tx,
                commands: cmds_rx,
                commands_sender: cmds_tx.clone(),
                db,
                timeouts_mng: timeouts,
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
                event = self.timeouts_mng.next_timeout() => {
                    self.handle_timeout(event).await
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
            BehaviourEvent::Identify(_event) => {
                // let identify::Event::Received()
                // self.swarm.behaviour_mut().gossipsub.add_explicit_peer(event.);
                Ok(())
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
        if let Err(err) = validate_gossipsub_msg(source, &msg) {
            debug!(reason=%err, "Got invalid signature from peer, rejecting message.");
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

        let new_event = self
            .insert_msg_if_not_exists_with_timeout(source, &msg, true)
            .await
            .context(RepositorySnafu)?;

        let event = Event::new(source, EventKind::GossipsubMsg(msg));

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

        if self.events.send(event).is_err() {
            whatever!("Events channel should never close");
        }

        Ok(())
    }

    /// Insert data received from event and reset timeout for this peer and
    /// deposit if it wasn't set before.
    ///
    /// Returns if the data was already presented or not.
    async fn insert_msg_if_not_exists_with_timeout(
        &mut self,
        source: PeerId,
        msg: &GossipsubMsg<DSP>,
        set_timeout: bool,
    ) -> DBResult<bool> {
        match &msg.kind {
            GossipsubMsgKind::GenesisInfo(info) => {
                let entry = self.db.get_genesis_info(source).await?;

                if entry.is_none() {
                    self.db
                        .set_genesis_info(
                            source,
                            GenesisInfoEntry {
                                entry: (info.pre_stake_outpoint, info.checkpoint_pubkeys.clone()),
                                signature: msg.signature.clone(),
                                key: msg.key.to_bytes().to_vec(),
                            },
                        )
                        .await?;

                    if set_timeout {
                        self.timeouts_mng
                            .set_genesis_timeout(source, self.config.next_stage_timeout);
                    }

                    return Ok(true);
                }
            }
            GossipsubMsgKind::Deposit { scope, kind } => match kind {
                GossipsubMsgDepositKind::Setup(dep) => {
                    let entry = self.db.get_deposit_setup(source, *scope).await?;

                    if entry.is_none() {
                        self.db
                            .set_deposit_setup(
                                source,
                                *scope,
                                DepositSetupEntry {
                                    payload: dep.payload.clone(),
                                    signature: msg.signature.clone(),
                                    key: msg.key.to_bytes().to_vec(),
                                },
                            )
                            .await?;

                        if set_timeout {
                            self.timeouts_mng.set_deposit_timeout(
                                source,
                                *scope,
                                self.config.next_stage_timeout,
                            );
                        }

                        return Ok(true);
                    }
                }
                GossipsubMsgDepositKind::Nonces(dep) => {
                    let entry = self.db.get_pub_nonces(source, *scope).await?;

                    if entry.is_none() {
                        self.db
                            .set_pub_nonces(
                                source,
                                *scope,
                                NoncesEntry {
                                    entry: dep.nonces.clone(),
                                    signature: msg.signature.clone(),
                                    key: msg.key.to_bytes().to_vec(),
                                },
                            )
                            .await?;
                        if set_timeout {
                            self.timeouts_mng.set_deposit_timeout(
                                source,
                                *scope,
                                self.config.next_stage_timeout,
                            );
                        }

                        return Ok(true);
                    }
                }
                GossipsubMsgDepositKind::Sigs(dep) => {
                    let entry = self.db.get_partial_signatures(source, *scope).await?;

                    if entry.is_none() {
                        self.db
                            .set_partial_signatures(
                                source,
                                *scope,
                                PartialSignaturesEntry {
                                    entry: dep.partial_sigs.clone(),
                                    signature: msg.signature.clone(),
                                    key: msg.key.to_bytes().to_vec(),
                                },
                            )
                            .await?;

                        return Ok(true);
                    }
                }
            },
        };

        Ok(false)
    }

    /// Handle command sent through channel by P2P implementation user.
    async fn handle_command(&mut self, cmd: Command<DSP>) -> P2PResult<()> {
        let local_peer_id = *self.swarm.local_peer_id();

        let kind = cmd.into_gossipsub_msg();
        let key = self.config.keypair.public();
        let signature = self.config.keypair.secret().sign(&kind.content());
        let msg = GossipsubMsg {
            kind,
            key: key.clone(),
            signature,
        };

        self.insert_msg_if_not_exists_with_timeout(
            local_peer_id,
            &msg,
            /* do net set timeout for local peer */ false,
        )
        .await
        .context(RepositorySnafu)?;

        // TODO(Velnbur): add retry mechanism later, instead of skipping the error
        let _ = self
            .swarm
            .behaviour_mut()
            .gossipsub
            .publish(TOPIC.hash(), msg.into_raw().encode_to_vec())
            .inspect_err(|err| debug!(%err, "Failed to publish msg through gossipsub"));

        Ok(())
    }

    /// P2P implementation keeps track of received stages for each peer and deposit, including the
    /// time P2P implementation received it last time. If after some "timeout" there were no
    /// messages for next state, P2P implementation request them directly from connected to it
    /// peers.
    ///
    /// This method implementats logic of requesting lost stages directly.
    #[instrument(skip(self))]
    async fn handle_timeout(&mut self, event: TimeoutEvent) -> P2PResult<()> {
        let TimeoutEvent::Deposit { operator_id, scope } = event else {
            // FIXME(Velnbur): handle deposit timeout too!
            return Ok(());
        };

        let peer_deposit_status = self
            .db
            .get_peer_deposit_status(operator_id, scope)
            .await
            .context(RepositorySnafu)?;

        let request_key = DepositRequestKey {
            scope: scope.to_byte_array().to_vec(),
            operator: operator_id.to_bytes(),
        };

        let body = match peer_deposit_status {
            PeerDepositState::PreSetup => get_message_request::Body::DepositSetup(request_key),
            PeerDepositState::Setup => get_message_request::Body::DepositNonce(request_key),
            PeerDepositState::Nonces => get_message_request::Body::DepositSigs(request_key),
            // NOTE(Velnbur): This should never happen, as after we got signatures from peer
            // it's the final stage, so we shouldn't establish timeout at this point at all.
            PeerDepositState::Sigs => {
                debug!("Tried to request next stage after timeout, but we already got everything");
                return Ok(());
            }
        };

        let behaviours = self.swarm.behaviour_mut();
        for (peer, _) in behaviours.gossipsub.all_peers() {
            let _req_id = behaviours.request_response.send_request(
                peer,
                GetMessageRequest {
                    body: Some(body.clone()),
                },
            );
        }

        Ok(())
    }

    #[instrument(skip(self, event))]
    async fn handle_request_response_event(
        &mut self,
        event: RequestResponseEvent<GetMessageRequest, GetMessageResponse>,
    ) -> P2PResult<()> {
        let RequestResponseEvent::Message { peer, message } = event else {
            return Ok(());
        };

        match message {
            request_response::Message::Request {
                request_id,
                request,
                channel,
            } => {
                let Some(req) = v1::GetMessageRequest::from_msg(request) else {
                    debug!(%peer, "Peer sent invalid get message request, disconnecting it");
                    let _ = self.swarm.disconnect_peer_id(peer);
                    return Ok(());
                };

                let msg = self.handle_get_message_request(req).await?;

                if msg.is_none() {
                    debug!(%request_id, "Have no needed data, requesting from neighbours");
                    return Ok(()); // TODO(NikitaMasych): launch recursive request.
                }

                let response = GetMessageResponse {
                    msg: vec![msg.unwrap()],
                };

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
                    debug!(%request_id, "Have no needed data, requesting from neighbours");
                    return Ok(()); // TODO(NikitaMasych): launch recursive request.
                }
                let mut all_messages_empty = true;
                for msg in response.msg.into_iter() {
                    if msg.body.is_none() {
                        continue;
                    }

                    // TODO: report/punish peer for invalid message?
                    match GossipsubMsg::from_proto(msg.clone()) {
                        Ok(msg) => {
                            if let Err(err) = validate_gossipsub_msg::<DSP>(peer, &msg) {
                                debug!(%peer, reason=%err, "Got invalid signature from peer, rejecting message.");
                                continue;
                            }
                        }
                        Err(err) => {
                            debug!(%peer, reason=%err, "Peer sent invalid message");
                            continue;
                        }
                    }

                    all_messages_empty = false;

                    let result = self.handle_get_message_response(peer, msg).await;

                    if let Err(err) = result {
                        if matches!(err, Error::Repository { .. }) {
                            return Err(err);
                        } else {
                            debug!(%peer, reason=%err, "Peer sent invalid message");
                        }
                    }
                }
                if all_messages_empty {
                    debug!(%request_id, "Have no needed data, requesting from neighbours");
                    return Ok(()); // TODO(NikitaMasych): launch recursive request.
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
            v1::GetMessageRequest::Genesis { operator_id } => {
                let info = self
                    .db
                    .get_genesis_info(operator_id)
                    .await
                    .context(RepositorySnafu)?;

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
                    key: v.key,
                })
            }
            v1::GetMessageRequest::ExchangeSession {
                scope,
                operator_id,
                kind,
            } => match kind {
                GetMessageRequestExchangeKind::Setup => {
                    let setup = self
                        .db
                        .get_deposit_setup(operator_id, scope)
                        .await
                        .context(RepositorySnafu)?;

                    setup.map(|v| proto::GossipsubMsg {
                        body: Some(Body::Setup(proto::DepositSetupExchange {
                            scope: scope.to_byte_array().to_vec(),
                            payload: v.payload.encode_to_vec(),
                        })),
                        signature: v.signature,
                        key: v.key,
                    })
                }
                GetMessageRequestExchangeKind::Nonces => {
                    let nonces = self
                        .db
                        .get_pub_nonces(operator_id, scope)
                        .await
                        .context(RepositorySnafu)?;

                    nonces.map(|v| proto::GossipsubMsg {
                        body: Some(Body::Nonce(proto::DepositNoncesExchange {
                            scope: scope.to_byte_array().to_vec(),
                            pub_nonces: v.entry.iter().map(|n| n.serialize().to_vec()).collect(),
                        })),
                        signature: v.signature,
                        key: v.key,
                    })
                }
                GetMessageRequestExchangeKind::Signatures => {
                    let sigs = self
                        .db
                        .get_partial_signatures(operator_id, scope)
                        .await
                        .context(RepositorySnafu)?;

                    sigs.map(|v| proto::GossipsubMsg {
                        body: Some(Body::Sigs(proto::DepositSignaturesExchange {
                            scope: scope.to_byte_array().to_vec(),
                            partial_sigs: v.entry.iter().map(|n| n.serialize().to_vec()).collect(),
                        })),
                        signature: v.signature,
                        key: v.key,
                    })
                }
            },
        };

        Ok(msg)
    }

    async fn handle_get_message_response(
        &mut self,
        peer: PeerId,
        msg: proto::GossipsubMsg,
    ) -> P2PResult<()> {
        match msg.body.unwrap() {
            Body::Setup(v) => {
                let scope = sha256::Hash::from_slice(&v.scope)
                    .whatever_context("failed to convert scope")?;
                let setup =
                    DepositSetup::from_proto_msg(&v).whatever_context("failed to convert setup")?;
                let entry = DepositSetupEntry {
                    payload: setup.payload,
                    signature: msg.signature,
                    key: msg.key,
                };
                self.db
                    .set_deposit_setup(peer, scope, entry)
                    .await
                    .context(RepositorySnafu)?
            }
            Body::Nonce(v) => {
                let scope = sha256::Hash::from_slice(&v.scope)
                    .whatever_context("failed to convert scope")?;
                let nonces = DepositNonces::from_proto_msg(&v)
                    .whatever_context("failed to convert nonces")?;
                let entry = NoncesEntry {
                    entry: nonces.nonces,
                    signature: msg.signature,
                    key: msg.key,
                };
                self.db
                    .set_pub_nonces(peer, scope, entry)
                    .await
                    .context(RepositorySnafu)?
            }
            Body::Sigs(v) => {
                let scope = sha256::Hash::from_slice(&v.scope)
                    .whatever_context("failed to convert scope")?;
                let sigs = DepositSigs::from_proto_msg(&v)
                    .whatever_context("failed to convert signatures")?;
                let entry = PartialSignaturesEntry {
                    entry: sigs.partial_sigs,
                    signature: msg.signature,
                    key: msg.key,
                };
                self.db
                    .set_partial_signatures(peer, scope, entry)
                    .await
                    .context(RepositorySnafu)?
            }
            Body::GenesisInfo(v) => {
                let info = v1::GenesisInfo::from_proto_msg(&v)
                    .whatever_context("failed to convert genesis info")?;
                let entry = GenesisInfoEntry {
                    entry: (info.pre_stake_outpoint, info.checkpoint_pubkeys),
                    signature: msg.signature,
                    key: msg.key,
                };
                self.db
                    .set_genesis_info(peer, entry)
                    .await
                    .context(RepositorySnafu)?
            }
        }

        Ok(())
    }
}

/// Checks gossip sub message for validity by protocol rules.
// NOTE(Velnbur): may be we should make this a method or move to separate repo, but I'm not sure.
fn validate_gossipsub_msg<DSP: prost::Message + Clone + Default>(
    source_peer: PeerId,
    msg: &GossipsubMsg<DSP>,
) -> Result<(), snafu::Whatever> {
    let content = msg.content();
    if !msg.key.verify(&content, &msg.signature) {
        whatever!("Invalid signature");
    }

    let msg_peer = PeerId::from_public_key(&libp2p::identity::PublicKey::from(msg.key.clone()));

    if msg_peer != source_peer {
        whatever!("Message signed by peer that is not sender");
    }

    Ok(())
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

/// Finish builder of swarm with paramets from config.
///
/// Again, macro is used as there is no way to specify this behaviour in
/// function, because `with_tcp` and `with_other_transport` return
/// completely different types that can't be generalized.
macro_rules! finish_swarm {
    ($builder:expr, $cfg:expr) => {
        $builder
            .whatever_context("failed to initialize transport")?
            .with_behaviour(|_| Behaviour::new(PROTOCOL_NAME, &$cfg.keypair, &$cfg.allowlist))
            .whatever_context("failed to initialize behaviour")?
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
