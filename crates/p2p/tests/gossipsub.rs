use bitcoin::{
    hashes::{sha256, Hash},
    OutPoint,
};
use common::Operator;
use futures::future::join_all;
use libp2p::{
    build_multiaddr,
    identity::{
        secp256k1::{Keypair as SecpKeypair, PublicKey},
        Keypair,
    },
    PeerId,
};
use snafu::whatever;
use strata_p2p::{
    commands::{Command, CommandKind},
    events::EventKind,
    swarm::handle::P2PHandle,
};
use strata_p2p_wire::p2p::v1::{GossipsubMsg, GossipsubMsgDepositKind, GossipsubMsgKind};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod common;

struct Setup {
    cancel: CancellationToken,
    operators: Vec<(P2PHandle<()>, PeerId)>,
    tasks: TaskTracker,
}

impl Setup {
    /// Spawn N operators that are connected "all-to-all" with handles to them, task tracker
    /// to stop control async tasks they are spawned in.
    pub async fn all_to_all(number: usize) -> Result<Self, snafu::Whatever> {
        let (keypairs, peer_ids, multiaddresses) =
            Self::setup_keys_ids_addrs_of_n_operators(number);

        let cancel = CancellationToken::new();
        let mut operators = Vec::new();

        for (idx, (keypair, addr)) in keypairs.iter().zip(&multiaddresses).enumerate() {
            let mut other_addrs = multiaddresses.clone();
            other_addrs.remove(idx);
            let mut other_keypairs = keypairs.clone();
            other_keypairs.remove(idx);
            let mut other_peerids = peer_ids.clone();
            other_peerids.remove(idx);

            let operator = Operator::new(
                keypair.clone(),
                other_peerids,
                other_addrs,
                addr.clone(),
                cancel.child_token(),
            )?;

            operators.push(operator);
        }

        let (handles, tasks) = Self::start_operators(operators).await;

        Ok(Self {
            cancel,
            tasks,
            operators: handles,
        })
    }

    /// Create N random keypairs, peer ids from them and sequantial in-memory
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
    async fn start_operators(
        mut operators: Vec<Operator>,
    ) -> (Vec<(P2PHandle<()>, PeerId)>, TaskTracker) {
        // wait until all of of them established connections and subscriptions
        join_all(
            operators
                .iter_mut()
                .map(|op| op.p2p.establish_connections())
                .collect::<Vec<_>>(),
        )
        .await;

        let mut handles = Vec::new();
        let tasks = TaskTracker::new();
        for operator in operators {
            let peer_id = operator.p2p.local_peer_id();
            tasks.spawn(operator.p2p.listen());
            handles.push((operator.handle, peer_id));
        }
        tasks.close();
        (handles, tasks)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_all_to_all_one_scope() -> Result<(), snafu::Whatever> {
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
async fn test_all_to_all_multiple_scopes() -> Result<(), snafu::Whatever> {
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
    operators: &mut [(P2PHandle<()>, PeerId)],
    operators_num: usize,
) -> Result<(), snafu::Whatever> {
    for (operator, _) in operators.iter() {
        operator
            .send_command(Command {
                key: generate_pk(),
                signature: vec![],
                kind: CommandKind::SendGenesisInfo {
                    pre_stake_outpoint: OutPoint::null(),
                    checkpoint_pubkeys: vec![],
                },
            })
            .await;
    }
    for (operator, peer_id) in operators.iter_mut() {
        // received genesis info from other n-1 operators
        for _ in 0..operators_num - 1 {
            let event = operator.next_event().await.unwrap();

            if !matches!(
                event.kind,
                EventKind::GossipsubMsg(GossipsubMsg {
                    kind: GossipsubMsgKind::GenesisInfo(_),
                    ..
                })
            ) {
                whatever!("Got event of other than 'genesis_info' kind: {:?}", event);
            }
            info!(to=%peer_id, from=%event.peer_id, "Got genesis info");
        }
        assert!(operator.events_is_empty());
    }

    Ok(())
}

async fn exchange_deposit_setup(
    operators: &mut [(P2PHandle<()>, PeerId)],
    operators_num: usize,
    scope_hash: sha256::Hash,
) -> Result<(), snafu::Whatever> {
    for (operator, _) in operators.iter() {
        operator
            .send_command(Command {
                key: generate_pk(),
                signature: vec![],
                kind: CommandKind::SendDepositSetup {
                    scope: scope_hash,
                    payload: (),
                },
            })
            .await;
    }
    for (operator, peer_id) in operators.iter_mut() {
        for _ in 0..operators_num - 1 {
            let event = operator.next_event().await.unwrap();
            // Skip messages from other scopes.
            if matches!(event.scope(), Some(scope) if scope != scope_hash) {
                continue;
            }
            if !matches!(
                event.kind,
                EventKind::GossipsubMsg(GossipsubMsg {
                    kind: GossipsubMsgKind::Deposit {
                        kind: GossipsubMsgDepositKind::Setup(_),
                        ..
                    },
                    ..
                })
            ) {
                whatever!("Got event of other than 'deposit_setup' kind: {:?}", event);
            }
            info!(to=%peer_id, from=%event.peer_id, "Got deposit setup");
        }
        assert!(operator.events_is_empty());
    }
    Ok(())
}

async fn exchange_deposit_nonces(
    operators: &mut [(P2PHandle<()>, PeerId)],
    operators_num: usize,
    scope_hash: sha256::Hash,
) -> Result<(), snafu::Whatever> {
    for (operator, _) in operators.iter() {
        operator
            .send_command(Command {
                key: generate_pk(),
                signature: vec![],
                kind: CommandKind::SendDepositNonces {
                    scope: scope_hash,
                    pub_nonces: vec![],
                },
            })
            .await;
    }
    for (operator, peer_id) in operators.iter_mut() {
        for _ in 0..operators_num - 1 {
            let event = operator.next_event().await.unwrap();
            // Skip messages from other scopes.
            if matches!(event.scope(), Some(scope) if scope != scope_hash) {
                continue;
            }
            if !matches!(
                event.kind,
                EventKind::GossipsubMsg(GossipsubMsg {
                    kind: GossipsubMsgKind::Deposit {
                        kind: GossipsubMsgDepositKind::Nonces(_),
                        ..
                    },
                    ..
                })
            ) {
                whatever!("Got event of other than 'deposit_nonces' kind: {:?}", event);
            }
            info!(to=%peer_id, from=%event.peer_id, "Got deposit nonces");
        }
        assert!(operator.events_is_empty());
    }
    Ok(())
}

async fn exchange_deposit_sigs(
    operators: &mut [(P2PHandle<()>, PeerId)],
    operators_num: usize,
    scope_hash: sha256::Hash,
) -> Result<(), snafu::Whatever> {
    for (operator, _) in operators.iter() {
        operator
            .send_command(Command {
                key: generate_pk(),
                signature: vec![],
                kind: CommandKind::SendPartialSignatures {
                    scope: scope_hash,
                    partial_sigs: vec![],
                },
            })
            .await;
    }

    for (operator, peer_id) in operators.iter_mut() {
        for _ in 0..operators_num - 1 {
            let event = operator.next_event().await.unwrap();
            // Skip messages from other scopes.
            if matches!(event.scope(), Some(scope) if scope != scope_hash) {
                continue;
            }
            if !matches!(
                event.kind,
                EventKind::GossipsubMsg(GossipsubMsg {
                    kind: GossipsubMsgKind::Deposit {
                        kind: GossipsubMsgDepositKind::Sigs(_),
                        ..
                    },
                    ..
                })
            ) {
                whatever!("Got event of other than 'deposit_nonces' kind: {:?}", event);
            }
            info!(to=%peer_id, from=%event.peer_id, "Got deposit sigs");
        }
        assert!(operator.events_is_empty());
    }

    Ok(())
}

fn generate_pk() -> PublicKey {
    let kp = SecpKeypair::generate();
    kp.public().clone()
}
