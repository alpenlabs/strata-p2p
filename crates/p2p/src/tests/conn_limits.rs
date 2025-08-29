//! Tests for connection limits.

#[cfg(feature = "byos")]
use std::sync::Arc;
use std::time::Duration;

use libp2p::{build_multiaddr, connection_limits::ConnectionLimits, identity::Keypair};
use tokio::{join, spawn, time::sleep};
use tokio_util::sync::CancellationToken;

#[cfg(feature = "byos")]
use crate::tests::common::MockApplicationSigner;
#[cfg(feature = "mem-conn-limits-abs")]
use crate::tests::common::SIXTEEN_GIBIBYTES;
use crate::tests::common::{User, init_tracing};
#[cfg(all(
    any(feature = "gossipsub", feature = "request-response"),
    not(feature = "byos")
))]
use crate::validator::DefaultP2PValidator;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conn_limits() -> anyhow::Result<()> {
    init_tracing();
    let cancel = CancellationToken::new();

    #[cfg(feature = "byos")]
    let user_app_keypair1 = Keypair::generate_ed25519();
    #[cfg(feature = "byos")]
    let user_app_keypair2 = Keypair::generate_ed25519();
    #[cfg(feature = "byos")]
    let user_app_keypair3 = Keypair::generate_ed25519();

    let user_transport_keypair1 = Keypair::generate_ed25519();
    let user_transport_keypair2 = Keypair::generate_ed25519();
    let user_transport_keypair3 = Keypair::generate_ed25519();

    let mut user1 = User::new(
        #[cfg(feature = "byos")]
        user_app_keypair1.clone(),
        user_transport_keypair1.clone(),
        vec![
            build_multiaddr!(Memory(5000u64 + 2u64)),
            build_multiaddr!(Memory(5000u64 + 3u64)),
        ], // Connect directly to existing users
        #[cfg(feature = "byos")]
        vec![user_app_keypair2.public(), user_app_keypair3.public()], // Allow all existing users
        vec![build_multiaddr!(Memory(5000u64 + 1u64))],
        cancel.child_token(),
        #[cfg(feature = "byos")]
        Arc::new(MockApplicationSigner::new(user_app_keypair1.clone())),
        #[cfg(all(
            any(feature = "gossipsub", feature = "request-response"),
            not(feature = "byos")
        ))]
        Box::new(DefaultP2PValidator),
        ConnectionLimits::default().with_max_established(Some(1u32)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0, // 100 %
    )?;

    let mut user2 = User::new(
        #[cfg(feature = "byos")]
        user_app_keypair2.clone(),
        user_transport_keypair2.clone(),
        vec![
            build_multiaddr!(Memory(5000u64 + 1u64)),
            build_multiaddr!(Memory(5000u64 + 3u64)),
        ], // Connect directly to existing users
        #[cfg(feature = "byos")]
        vec![user_app_keypair1.public(), user_app_keypair3.public()], // Allow all existing users
        vec![build_multiaddr!(Memory(5000u64 + 2u64))],
        cancel.child_token(),
        #[cfg(feature = "byos")]
        Arc::new(MockApplicationSigner::new(user_app_keypair2.clone())),
        #[cfg(all(
            any(feature = "gossipsub", feature = "request-response"),
            not(feature = "byos")
        ))]
        Box::new(DefaultP2PValidator),
        ConnectionLimits::default().with_max_established(Some(1u32)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0, // 100 %
    )?;

    let mut user3 = User::new(
        #[cfg(feature = "byos")]
        user_app_keypair3.clone(),
        user_transport_keypair3.clone(),
        vec![
            build_multiaddr!(Memory(5000u64 + 1u64)),
            build_multiaddr!(Memory(5000u64 + 2u64)),
        ], // Connect directly to existing users
        #[cfg(feature = "byos")]
        vec![user_app_keypair1.public(), user_app_keypair2.public()], // Allow all existing users
        vec![build_multiaddr!(Memory(5000u64 + 3u64))],
        cancel.child_token(),
        #[cfg(feature = "byos")]
        Arc::new(MockApplicationSigner::new(user_app_keypair3.clone())),
        #[cfg(all(
            any(feature = "gossipsub", feature = "request-response"),
            not(feature = "byos")
        ))]
        Box::new(DefaultP2PValidator),
        ConnectionLimits::default().with_max_established(Some(1u32)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0, // 100 %
    )?;

    let task1 = spawn(async move {
        user1.p2p.establish_connections().await;
        user1.p2p.listen().await
    });
    let task2 = spawn(async move {
        user2.p2p.establish_connections().await;
        user2.p2p.listen().await
    });
    let task3 = spawn(async move {
        user3.p2p.establish_connections().await;
        user3.p2p.listen().await
    });

    sleep(Duration::from_secs(2)).await;

    let amount_of_connections = user1.command.get_connected_peers(None).await.len()
        + user2.command.get_connected_peers(None).await.len()
        + user3.command.get_connected_peers(None).await.len();
    assert!((amount_of_connections == 2) || (amount_of_connections == 0));

    // Clean up
    cancel.cancel();
    let _ = join!(task1, task2, task3);

    Ok(())
}
