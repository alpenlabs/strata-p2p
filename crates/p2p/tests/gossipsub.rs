use anyhow::bail;
use bitcoin::{
    hashes::{sha256, Hash},
    OutPoint,
};
use common::Operator;
use futures::future::join_all;
use libp2p::{
    build_multiaddr,
    identity::{secp256k1::Keypair as SecpKeypair, Keypair},
    PeerId,
};
use strata_p2p::{
    commands::{Command, UnsignedPublishMessage},
    events::Event,
    swarm::handle::P2PHandle,
};
use strata_p2p_db::{sled::AsyncDB, GenesisInfoEntry, RepositoryExt};
use strata_p2p_types::OperatorPubKey;
use strata_p2p_wire::p2p::v1::{GetMessageRequest, GossipsubMsgDepositKind, GossipsubMsgKind};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod common;

/// Auxiliary structure to control operators from outside.
struct OperatorHandle {
    handle: P2PHandle<()>,
    peer_id: PeerId,
    kp: SecpKeypair,
    db: AsyncDB, // We include DB here to manipulate internal data and flow mechanics.
}

struct Setup {
    cancel: CancellationToken,
    operators: Vec<OperatorHandle>,
    tasks: TaskTracker,
}

impl Setup {
    /// Spawn N operators that are connected "all-to-all" with handles to them, task tracker
    /// to stop control async tasks they are spawned in.
    pub async fn all_to_all(number: usize) -> anyhow::Result<Self> {
        let (keypairs, peer_ids, multiaddresses) =
            Self::setup_keys_ids_addrs_of_n_operators(number);

        let cancel = CancellationToken::new();
        let mut operators = Vec::new();
        let signers_allowlist: Vec<OperatorPubKey> = keypairs
            .clone()
            .into_iter()
            .map(|kp| kp.public().clone().into())
            .collect();

        for (idx, (keypair, addr)) in keypairs.iter().zip(&multiaddresses).enumerate() {
            let mut other_addrs = multiaddresses.clone();
            other_addrs.remove(idx);
            let mut other_peerids = peer_ids.clone();
            other_peerids.remove(idx);

            let operator = Operator::new(
                keypair.clone(),
                other_peerids,
                other_addrs,
                addr.clone(),
                cancel.child_token(),
                signers_allowlist.clone(),
            )?;

            operators.push(operator);
        }

        let (operators, tasks) = Self::start_operators(operators).await;

        Ok(Self {
            cancel,
            tasks,
            operators,
        })
    }

    /// Create N random keypairs, peer ids from them and sequential in-memory
    /// addresses.
    fn setup_keys_ids_addrs_of_n_operators(
        number: usize,
    ) -> (Vec<SecpKeypair>, Vec<PeerId>, Vec<libp2p::Multiaddr>) {
        let keypairs = (0..number)
            .map(|_| SecpKeypair::generate())
            .collect::<Vec<_>>();
        let peer_ids = keypairs
            .iter()
            .map(|key| PeerId::from_public_key(&Keypair::from(key.clone()).public()))
            .collect::<Vec<_>>();
        let multiaddresses = (1..(keypairs.len() + 1) as u16)
            .map(|idx| build_multiaddr!(Memory(idx)))
            .collect::<Vec<_>>();
        (keypairs, peer_ids, multiaddresses)
    }

    /// Wait until all operators established connections with other operators,
    /// and then spawn [`P2P::listen`]s in separate tasks using [`TaskTracker`].
    async fn start_operators(mut operators: Vec<Operator>) -> (Vec<OperatorHandle>, TaskTracker) {
        // wait until all of them established connections and subscriptions
        join_all(
            operators
                .iter_mut()
                .map(|op| op.p2p.establish_connections())
                .collect::<Vec<_>>(),
        )
        .await;

        let mut levers = Vec::new();
        let tasks = TaskTracker::new();
        for operator in operators {
            let peer_id = operator.p2p.local_peer_id();
            tasks.spawn(operator.p2p.listen());

            levers.push(OperatorHandle {
                handle: operator.handle,
                peer_id,
                kp: operator.kp,
                db: operator.db,
            });
        }

        tasks.close();
        (levers, tasks)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_all_to_all_one_scope() -> anyhow::Result<()> {
    const OPERATORS_NUM: usize = 4;

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let Setup {
        mut operators,
        cancel,
        tasks,
    } = Setup::all_to_all(OPERATORS_NUM).await?;

    let scope_hash = sha256::Hash::hash(b"scope");

    exchange_genesis_info(&mut operators, OPERATORS_NUM).await?;
    exchange_deposit_setup(&mut operators, OPERATORS_NUM, scope_hash).await?;
    exchange_deposit_nonces(&mut operators, OPERATORS_NUM, scope_hash).await?;
    exchange_deposit_sigs(&mut operators, OPERATORS_NUM, scope_hash).await?;

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_request_response() -> anyhow::Result<()> {
    const OPERATORS_NUM: usize = 4;

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let Setup {
        mut operators,
        cancel,
        tasks,
    } = Setup::all_to_all(OPERATORS_NUM).await?;

    // last operator won't send his info to others
    exchange_genesis_info(&mut operators[..OPERATORS_NUM - 1], OPERATORS_NUM - 1).await?;

    // create command to request info from the last operator
    let operator_pk: OperatorPubKey = operators[OPERATORS_NUM - 1].kp.public().clone().into();
    let command = Command::<()>::RequestMessage(GetMessageRequest::Genesis {
        operator_pk: operator_pk.clone(),
    });

    // put data in the last operator, so that he can respond it
    match mock_genesis_info(&operators[OPERATORS_NUM - 1].kp.clone()) {
        Command::PublishMessage(msg) => match msg.msg {
            UnsignedPublishMessage::GenesisInfo {
                pre_stake_outpoint,
                checkpoint_pubkeys,
            } => {
                let entry = GenesisInfoEntry {
                    entry: (pre_stake_outpoint, checkpoint_pubkeys),
                    signature: msg.signature,
                    key: msg.key,
                };
                <AsyncDB as RepositoryExt<()>>::set_genesis_info_if_not_exists::<'_, '_>(
                    &operators[OPERATORS_NUM - 1].db,
                    entry,
                )
                .await?;
            }
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }

    operators[0].handle.send_command(command).await;

    let event = operators[0].handle.next_event().await?;

    match event {
        Event::ReceivedMessage(msg)
            if matches!(msg.kind, GossipsubMsgKind::GenesisInfo(_)) && msg.key == operator_pk =>
        {
            info!("Got genesis info from the last operator")
        }

        _ => bail!("Got event other than 'genesis_info' - {:?}", event),
    }

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_all_to_all_multiple_scopes() -> anyhow::Result<()> {
    const OPERATORS_NUM: usize = 10;

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let Setup {
        mut operators,
        cancel,
        tasks,
    } = Setup::all_to_all(OPERATORS_NUM).await?;

    exchange_genesis_info(&mut operators, OPERATORS_NUM).await?;

    let scopes = (0..10)
        .map(|i| sha256::Hash::hash(format!("scope{}", i).as_bytes()))
        .collect::<Vec<_>>();

    for scope in &scopes {
        exchange_deposit_setup(&mut operators, OPERATORS_NUM, *scope).await?;
    }
    for scope in &scopes {
        exchange_deposit_nonces(&mut operators, OPERATORS_NUM, *scope).await?;
    }
    for scope in &scopes {
        exchange_deposit_sigs(&mut operators, OPERATORS_NUM, *scope).await?;
    }

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}

async fn exchange_genesis_info(
    operators: &mut [OperatorHandle],
    operators_num: usize,
) -> anyhow::Result<()> {
    for operator in operators.iter() {
        operator
            .handle
            .send_command(mock_genesis_info(&operator.kp))
            .await;
    }
    for operator in operators.iter_mut() {
        // received genesis info from other n-1 operators
        for _ in 0..operators_num - 1 {
            let event = operator.handle.next_event().await?;

            match event {
                Event::ReceivedMessage(msg)
                    if matches!(msg.kind, GossipsubMsgKind::GenesisInfo(_)) =>
                {
                    info!(to=%operator.peer_id, "Got genesis info")
                }

                _ => bail!("Got event other than 'genesis_info' - {:?}", event),
            }
        }

        assert!(operator.handle.events_is_empty());
    }

    Ok(())
}

async fn exchange_deposit_setup(
    operators: &mut [OperatorHandle],
    operators_num: usize,
    scope_hash: sha256::Hash,
) -> anyhow::Result<()> {
    for operator in operators.iter() {
        operator
            .handle
            .send_command(mock_deposit_setup(&operator.kp, scope_hash))
            .await;
    }
    for operator in operators.iter_mut() {
        for _ in 0..operators_num - 1 {
            let event = operator.handle.next_event().await?;
            match event {
                Event::ReceivedMessage(msg)
                    if matches!(
                        msg.kind,
                        GossipsubMsgKind::Deposit {
                            kind: GossipsubMsgDepositKind::Setup(_),
                            ..
                        },
                    ) =>
                {
                    info!(to=%operator.peer_id, "Got deposit setup")
                }
                _ => bail!("Got event other than 'deposit_setup' - {:?}", event),
            }
        }
        assert!(operator.handle.events_is_empty());
    }
    Ok(())
}

async fn exchange_deposit_nonces(
    operators: &mut [OperatorHandle],
    operators_num: usize,
    scope_hash: sha256::Hash,
) -> anyhow::Result<()> {
    for operator in operators.iter() {
        operator
            .handle
            .send_command(mock_deposit_nonces(&operator.kp, scope_hash))
            .await;
    }
    for operator in operators.iter_mut() {
        for _ in 0..operators_num - 1 {
            let event = operator.handle.next_event().await?;
            match event {
                Event::ReceivedMessage(msg)
                    if matches!(
                        msg.kind,
                        GossipsubMsgKind::Deposit {
                            kind: GossipsubMsgDepositKind::Nonces(_),
                            ..
                        },
                    ) =>
                {
                    info!(to=%operator.peer_id, "Got deposit nonces")
                }
                _ => bail!("Got event other than 'deposit_nonces' - {:?}", event),
            }
        }
        assert!(operator.handle.events_is_empty());
    }
    Ok(())
}

async fn exchange_deposit_sigs(
    operators: &mut [OperatorHandle],
    operators_num: usize,
    scope_hash: sha256::Hash,
) -> anyhow::Result<()> {
    for operator in operators.iter() {
        operator
            .handle
            .send_command(mock_deposit_sigs(&operator.kp, scope_hash))
            .await;
    }

    for operator in operators.iter_mut() {
        for _ in 0..operators_num - 1 {
            let event = operator.handle.next_event().await?;
            match event {
                Event::ReceivedMessage(msg)
                    if matches!(
                        msg.kind,
                        GossipsubMsgKind::Deposit {
                            kind: GossipsubMsgDepositKind::Sigs(_),
                            ..
                        },
                    ) =>
                {
                    info!(to=%operator.peer_id, "Got deposit sigs")
                }
                _ => bail!("Got event other than 'deposit_sigs' - {:?}", event),
            }
        }
        assert!(operator.handle.events_is_empty());
    }

    Ok(())
}

fn mock_genesis_info(kp: &SecpKeypair) -> Command<()> {
    let kind = UnsignedPublishMessage::GenesisInfo {
        pre_stake_outpoint: OutPoint::null(),
        checkpoint_pubkeys: vec![],
    };
    kind.sign_secp256k1(kp).into()
}

fn mock_deposit_setup(kp: &SecpKeypair, scope_hash: sha256::Hash) -> Command<()> {
    let unsigned = UnsignedPublishMessage::DepositSetup {
        scope: scope_hash,
        payload: (),
    };
    unsigned.sign_secp256k1(kp).into()
}

fn mock_deposit_nonces(kp: &SecpKeypair, scope_hash: sha256::Hash) -> Command<()> {
    let unsigned = UnsignedPublishMessage::DepositNonces {
        scope: scope_hash,
        pub_nonces: vec![],
    };
    unsigned.sign_secp256k1(kp).into()
}

fn mock_deposit_sigs(kp: &SecpKeypair, scope_hash: sha256::Hash) -> Command<()> {
    let unsigned = UnsignedPublishMessage::PartialSignatures {
        scope: scope_hash,
        partial_sigs: vec![],
    };
    unsigned.sign_secp256k1(kp).into()
}
