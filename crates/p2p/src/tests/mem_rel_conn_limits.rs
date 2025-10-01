//! Tests for denying new connections when used RAM by current process has reached a specified
//! threshold, which in this test is 0 % .

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
async fn mem_conn_limits_rel() -> anyhow::Result<()> {
    init_tracing();
    let cancel = CancellationToken::new();

    #[cfg(feature = "byos")]
    let user_app_keypair1 = Keypair::generate_ed25519();
    #[cfg(feature = "byos")]
    let user_app_keypair2 = Keypair::generate_ed25519();

    let user_transport_keypair1 = Keypair::generate_ed25519();
    let user_transport_keypair2 = Keypair::generate_ed25519();

    let mut user1 = User::new(
        #[cfg(feature = "byos")]
        user_app_keypair1.clone(),
        user_transport_keypair1.clone(),
        vec![build_multiaddr!(Memory(7000u64 + 2u64))], // Connect directly to existing users
        #[cfg(feature = "byos")]
        vec![user_app_keypair2.public()], // Allow all existing users
        vec![build_multiaddr!(Memory(7000u64 + 1u64))],
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
        0.0, // 0 %
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
    )?;

    let mut user2 = User::new(
        #[cfg(feature = "byos")]
        user_app_keypair2.clone(),
        user_transport_keypair2.clone(),
        vec![build_multiaddr!(Memory(7000u64 + 1u64))], // Connect directly to existing users
        #[cfg(feature = "byos")]
        vec![user_app_keypair1.public()], // Allow all existing users
        vec![build_multiaddr!(Memory(7000u64 + 2u64))],
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
        0.0, // 0 %
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
    )?;

    let task1 = spawn(async move {
        user1.p2p.establish_connections().await;
        user1.p2p.listen().await
    });
    let task2 = spawn(async move {
        user2.p2p.establish_connections().await;
        user2.p2p.listen().await
    });

    sleep(Duration::from_secs(2)).await;

    assert_eq!(
        user1.command.get_connected_peers(None).await.len()
            + user2.command.get_connected_peers(None).await.len(),
        0
    );

    // Clean up
    cancel.cancel();
    let _ = join!(task1, task2);

    Ok(())
}
