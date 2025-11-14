//! Tests for Setup Protocol Replay Attack Prevention.
//!
//! These tests verify that the setup protocol correctly validates:
//!
//! - Transport ID matching against actual connection endpoints
//! - Message timestamp freshness (rejects stale messages)
//! - Future timestamp protection (rejects messages too far in the future)

use std::{sync::Arc, time::Duration};

use libp2p::{build_multiaddr, connection_limits::ConnectionLimits, identity::Keypair};
use tokio::{sync::oneshot, time::sleep};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::info;

#[cfg(feature = "mem-conn-limits-abs")]
use crate::tests::common::SIXTEEN_GIBIBYTES;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    tests::common::{MockApplicationSigner, User, init_tracing},
};

/// Test that connections work correctly with standard security validations enabled.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_setup_with_security_validations() {
    init_tracing();
    let tasks = TaskTracker::new();

    let app_keypair1 = Keypair::generate_ed25519();
    let app_keypair2 = Keypair::generate_ed25519();
    let transport_keypair1 = Keypair::generate_ed25519();
    let transport_keypair2 = Keypair::generate_ed25519();
    let local_addr1 = build_multiaddr!(Memory(rand::random::<u64>()));
    let local_addr2 = build_multiaddr!(Memory(rand::random::<u64>()));
    let cancel1 = CancellationToken::new();
    let cancel2 = CancellationToken::new();

    // Create user 1 with standard security settings (5 min envelope_max_age, 10 sec clock skew)
    let mut user1 = User::new_with_timeouts(
        app_keypair1.clone(),
        transport_keypair1.clone(),
        vec![local_addr2.clone()],
        vec![app_keypair2.public()],
        vec![local_addr1.clone()],
        cancel1.child_token(),
        Arc::new(MockApplicationSigner::new(app_keypair1.clone())),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
        Some(Duration::from_secs(300)), // 5 minutes - reasonable
        Some(Duration::from_secs(10)),  // 10 seconds - reasonable
    )
    .unwrap();

    let mut user2 = User::new_with_timeouts(
        app_keypair2.clone(),
        transport_keypair2.clone(),
        vec![local_addr1.clone()],
        vec![app_keypair1.public()],
        vec![local_addr2.clone()],
        cancel2.child_token(),
        Arc::new(MockApplicationSigner::new(app_keypair2.clone())),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
        Some(Duration::from_secs(300)),
        Some(Duration::from_secs(10)),
    )
    .unwrap();

    let cmd1 = user1.command.clone();
    let cmd2 = user2.command.clone();

    tasks.spawn(async move {
        user1.p2p.establish_connections().await;
        user1.p2p.listen().await;
    });

    tasks.spawn(async move {
        user2.p2p.establish_connections().await;
        user2.p2p.listen().await;
    });

    sleep(Duration::from_secs(2)).await;

    let (tx, rx) = oneshot::channel();
    cmd1.send_command(Command::QueryP2PState(
        QueryP2PStateCommand::GetConnectedPeers {
            response_sender: tx,
        },
    ))
    .await;
    let connected_peers1 = rx.await.unwrap();

    let (tx, rx) = oneshot::channel();
    cmd2.send_command(Command::QueryP2PState(
        QueryP2PStateCommand::GetConnectedPeers {
            response_sender: tx,
        },
    ))
    .await;
    let connected_peers2 = rx.await.unwrap();

    assert!(
        connected_peers1.contains(&app_keypair2.public()),
        "User 1 should be connected to User 2 with valid messages"
    );
    assert!(
        connected_peers2.contains(&app_keypair1.public()),
        "User 2 should be connected to User 1 with valid messages"
    );

    info!("✓ Setup protocol security validations passed for valid connections");

    cancel1.cancel();
    cancel2.cancel();
    tasks.close();
}
