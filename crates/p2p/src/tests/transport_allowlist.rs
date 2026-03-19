use std::time::Duration;

use libp2p::{PeerId, connection_limits::ConnectionLimits};
use tokio::time::sleep;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::common::{Setup, SetupInitialData, User, init_tracing};
use crate::{
    commands::Command,
    swarm::{P2P, handle::CommandHandle},
    validator::DefaultP2PValidator,
};

struct PeerHarnessUser {
    peer_id: PeerId,
    p2p: Option<P2P>,
    command: CommandHandle,
}

struct PeerConnectionState {
    victim_connected: bool,
    peer_connected: bool,
}

fn new_user(
    setup: &SetupInitialData,
    index: usize,
    transport_allowlist: Option<Vec<PeerId>>,
    cancel: &CancellationToken,
) -> anyhow::Result<User> {
    User::new(
        setup.transport_keypairs[index].clone(),
        vec![],
        transport_allowlist,
        vec![setup.multiaddresses[index].clone()],
        cancel.child_token(),
        Box::new(DefaultP2PValidator),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        super::common::SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
    )
}

async fn dial_peers_and_collect_connection_states<F>(
    peer_count: usize,
    build_transport_allowlist: F,
) -> anyhow::Result<Vec<PeerConnectionState>>
where
    F: FnOnce(&[PeerId]) -> Option<Vec<PeerId>>,
{
    let setup = Setup::setup_keys_ids_addrs_of_n_users(peer_count + 1);
    let cancel = CancellationToken::new();
    let peer_ids = setup.peer_ids[1..].to_vec();

    let victim = new_user(&setup, 0, build_transport_allowlist(&peer_ids), &cancel)?;
    let mut peers = (1..=peer_count)
        .map(|index| {
            let User { p2p, command, .. } = new_user(&setup, index, None, &cancel)?;
            Ok(PeerHarnessUser {
                peer_id: setup.peer_ids[index],
                p2p: Some(p2p),
                command,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let victim_peer_id = setup.peer_ids[0];
    let victim_addr = setup.multiaddresses[0].clone();

    let User {
        p2p: victim_p2p,
        command: victim_command,
        ..
    } = victim;

    let tasks = TaskTracker::new();
    tasks.spawn(victim_p2p.listen());
    for peer in &mut peers {
        tasks.spawn(
            peer.p2p
                .take()
                .expect("test peer listener should only be taken once")
                .listen(),
        );
    }
    tasks.close();

    sleep(Duration::from_millis(200)).await;

    for peer in &peers {
        peer.command
            .send_command(Command::ConnectToPeer {
                transport_id: victim_peer_id,
                addresses: vec![victim_addr.clone()],
            })
            .await;
    }

    sleep(Duration::from_secs(1)).await;

    let mut connection_states = Vec::with_capacity(peer_count);
    for peer in &peers {
        connection_states.push(PeerConnectionState {
            victim_connected: victim_command.is_connected(&peer.peer_id, None).await,
            peer_connected: peer.command.is_connected(&victim_peer_id, None).await,
        });
    }

    cancel.cancel();
    tasks.wait().await;

    Ok(connection_states)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn allows_peer_when_transport_allowlist_is_none() -> anyhow::Result<()> {
    init_tracing();

    let states = dial_peers_and_collect_connection_states(1, |_| None).await?;
    let state = &states[0];

    assert!(state.victim_connected);
    assert!(state.peer_connected);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_peer_when_transport_allowlist_is_empty() -> anyhow::Result<()> {
    init_tracing();

    let states = dial_peers_and_collect_connection_states(1, |_| Some(vec![])).await?;
    let state = &states[0];

    assert!(!state.victim_connected);
    assert!(!state.peer_connected);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_peer_not_in_transport_allowlist() -> anyhow::Result<()> {
    init_tracing();

    let states =
        dial_peers_and_collect_connection_states(1, |_| Some(vec![PeerId::random()])).await?;
    let state = &states[0];

    assert!(!state.victim_connected);
    assert!(!state.peer_connected);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn allows_only_peers_in_transport_allowlist() -> anyhow::Result<()> {
    init_tracing();

    const PEER_COUNT: usize = 2;
    const ALLOWED_PEER: usize = 0;
    const REJECTED_PEER: usize = 1;
    let states = dial_peers_and_collect_connection_states(PEER_COUNT, |peer_ids| {
        Some(vec![peer_ids[ALLOWED_PEER]])
    })
    .await?;

    assert_eq!(states.len(), 2);
    assert!(states[ALLOWED_PEER].victim_connected);
    assert!(states[ALLOWED_PEER].peer_connected);
    assert!(!states[REJECTED_PEER].victim_connected);
    assert!(!states[REJECTED_PEER].peer_connected);

    Ok(())
}
