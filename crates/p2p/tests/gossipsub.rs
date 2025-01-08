use std::time::Duration;

use bitcoin::{
    hashes::{sha256, Hash},
    OutPoint,
};
use common::Operator;
use libp2p::{
    build_multiaddr,
    identity::{secp256k1::Keypair as SecpKeypair, Keypair},
    PeerId,
};
use snafu::whatever;
use strata_p2p::{events::EventKind, swarm::handle::P2PHandle};
use strata_p2p_wire::p2p::v1::{GossipsubMsg, GossipsubMsgDepositKind, GossipsubMsgKind};
use tokio::time::sleep;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod common;

struct Setup {
    cancel: CancellationToken,
    operators: Vec<P2PHandle<()>>,
    tasks: TaskTracker,
}

impl Setup {
    pub fn all_to_all(number: usize) -> Result<Self, snafu::Whatever> {
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

        let mut handles = Vec::new();
        let tasks = TaskTracker::new();
        for operator in operators {
            tasks.spawn(operator.p2p.listen());
            handles.push(operator.handle);
        }
        tasks.close();

        Ok(Self {
            cancel,
            tasks,
            operators: handles,
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 5)]
async fn test_all_to_all_init() -> Result<(), snafu::Whatever> {
    const OPERATORS_NUM: usize = 4;

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let Setup {
        mut operators,
        cancel,
        tasks,
    } = Setup::all_to_all(OPERATORS_NUM)?;

    let scope_hash = sha256::Hash::hash(b"scope");

    sleep(Duration::from_secs(5)).await;
    for operator in &operators {
        operator.send_genesis_info(OutPoint::null(), vec![]).await;
    }
    for (idx, operator) in operators.iter_mut().enumerate() {
        // received genesis info from other n-1 operators
        for _ in 0..OPERATORS_NUM - 1 {
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
            info!(to=idx, from=%event.peer_id, "Got genesis info");
        }

        assert!(operator.events_is_empty());
    }

    /* Exchange deposit setups */
    for operator in &operators {
        operator.send_deposit_setup(scope_hash, ()).await;
    }
    for (idx, operator) in operators.iter_mut().enumerate() {
        for _ in 0..OPERATORS_NUM - 1 {
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
            info!(to=idx, from=%event.peer_id, "Got deposit setup");
        }

        assert!(operator.events_is_empty());
    }

    /* Exchange nonces */
    for operator in &operators {
        operator.send_deposit_nonces(scope_hash, vec![]).await;
    }
    for (idx, operator) in operators.iter_mut().enumerate() {
        for _ in 0..OPERATORS_NUM - 1 {
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
            info!(to=idx, from=%event.peer_id, "Got deposit nonces");
        }

        assert!(operator.events_is_empty());
    }

    /* Exchange sigs */
    for operator in &operators {
        operator.send_deposit_sigs(scope_hash, vec![]).await;
    }
    for (idx, operator) in operators.iter_mut().enumerate() {
        for _ in 0..OPERATORS_NUM - 1 {
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
            info!(to=idx, from=%event.peer_id, "Got deposit sigs");
        }
        assert!(operator.events_is_empty());
    }

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}
