use std::time::Duration;

use libp2p::{PeerId, connection_limits::ConnectionLimits};
use tokio::time::sleep;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::common::{Setup, User, init_tracing};
use crate::{commands::Command, validator::DefaultP2PValidator};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejects_peer_not_in_transport_allowlist() -> anyhow::Result<()> {
    init_tracing();

    let setup = Setup::setup_keys_ids_addrs_of_n_users(2);
    let cancel = CancellationToken::new();

    let victim = User::new(
        setup.transport_keypairs[0].clone(),
        vec![],
        Some(vec![PeerId::random()]),
        vec![setup.multiaddresses[0].clone()],
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
    )?;

    let attacker = User::new(
        setup.transport_keypairs[1].clone(),
        vec![],
        None,
        vec![setup.multiaddresses[1].clone()],
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
    )?;

    let victim_peer_id = setup.peer_ids[0];
    let attacker_peer_id = setup.peer_ids[1];
    let victim_addr = setup.multiaddresses[0].clone();

    let User {
        p2p: victim_p2p,
        command: victim_command,
        ..
    } = victim;
    let User {
        p2p: attacker_p2p,
        command: attacker_command,
        ..
    } = attacker;

    let tasks = TaskTracker::new();
    tasks.spawn(victim_p2p.listen());
    tasks.spawn(attacker_p2p.listen());
    tasks.close();

    sleep(Duration::from_millis(200)).await;

    attacker_command
        .send_command(Command::ConnectToPeer {
            transport_id: victim_peer_id,
            addresses: vec![victim_addr],
        })
        .await;

    sleep(Duration::from_secs(1)).await;

    assert!(!victim_command.is_connected(&attacker_peer_id, None).await);
    assert!(!attacker_command.is_connected(&victim_peer_id, None).await);

    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
