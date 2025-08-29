//! Tests for the setup phase of P2P connections where signature is invalid.

use std::{sync::Arc, time::Duration};

use libp2p::{build_multiaddr, connection_limits::ConnectionLimits, identity::Keypair};
use tokio::{sync::oneshot, time::sleep};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::info;

#[cfg(feature = "mem-conn-limits-abs")]
use crate::tests::common::SIXTEEN_GIBIBYTES;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    signer::ApplicationSigner,
    tests::common::{MockApplicationSigner, User, init_tracing},
};

#[derive(Debug, Clone)]
struct BadApplicationSigner;

impl BadApplicationSigner {
    pub(crate) fn new(_app_keypair: Keypair) -> Self {
        Self
    }
}

impl ApplicationSigner for BadApplicationSigner {
    fn sign(&self, _message: &[u8]) -> Result<[u8; 64], Box<dyn std::error::Error + Send + Sync>> {
        let signature = [0x02; 64];
        Ok(signature)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_setup_with_invalid_signature() {
    init_tracing();
    let tasks = TaskTracker::new();

    let app_keypair_good1 = Keypair::generate_ed25519();
    let app_keypair_good2 = Keypair::generate_ed25519();
    let transport_keypair_good1 = Keypair::generate_ed25519();
    let transport_keypair_good2 = Keypair::generate_ed25519();
    let local_addr_good1 = build_multiaddr!(Memory(rand::random::<u64>()));
    let local_addr_good2 = build_multiaddr!(Memory(rand::random::<u64>()));
    let cancel_good1 = CancellationToken::new();
    let cancel_good2 = CancellationToken::new();

    let mut good_user1 = User::new(
        app_keypair_good1.clone(),
        transport_keypair_good1.clone(),
        vec![local_addr_good2.clone()],
        vec![app_keypair_good2.public()],
        vec![local_addr_good1.clone()],
        cancel_good1.child_token(),
        Arc::new(MockApplicationSigner::new(app_keypair_good1.clone())),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0, // 100 %
    )
    .unwrap();

    let mut good_user2 = User::new(
        app_keypair_good2.clone(),
        transport_keypair_good2.clone(),
        vec![local_addr_good1.clone()],
        vec![app_keypair_good1.public()],
        vec![local_addr_good2.clone()],
        cancel_good2.child_token(),
        Arc::new(MockApplicationSigner::new(app_keypair_good2.clone())),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0, // 100 %
    )
    .unwrap();

    let good_peer_id1 = good_user1.transport_keypair.public().to_peer_id();
    let good_command_handle1 = good_user1.command.clone();

    tasks.spawn(async move {
        good_user1.p2p.establish_connections().await;
        good_user1.p2p.listen().await;
    });

    let good_command_handle2 = good_user2.command.clone();

    tasks.spawn(async move {
        good_user2.p2p.establish_connections().await;
        good_user2.p2p.listen().await;
    });

    let app_keypair_bad = Keypair::generate_ed25519();
    let transport_keypair_bad = Keypair::generate_ed25519();
    let local_addr_bad = build_multiaddr!(Memory(rand::random::<u64>()));
    let cancel_bad = CancellationToken::new();

    let bad_user = User::new(
        app_keypair_bad.clone(),
        transport_keypair_bad.clone(),
        vec![],                           // connect_to
        vec![app_keypair_good1.public()], // allowlist
        vec![local_addr_bad.clone()],     // listening_addrs
        cancel_bad.child_token(),
        Arc::new(BadApplicationSigner::new(app_keypair_bad.clone())),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0, // 100 %
    )
    .unwrap();

    let bad_peer_id = bad_user.transport_keypair.public().to_peer_id();
    let bad_command_handle = bad_user.command.clone();

    tasks.spawn(async move {
        bad_user.p2p.listen().await;
    });

    // Give some time for listeners to start
    sleep(Duration::from_secs(1)).await;

    info!(
        "Good user ({good_peer_id1}) attempting to connect to bad user ({bad_peer_id}) at {local_addr_bad}"
    );
    good_command_handle1
        .send_command(Command::ConnectToPeer {
            app_public_key: app_keypair_bad.public(),
            addresses: vec![local_addr_bad.clone()],
        })
        .await;

    info!(
        "Bad user ({bad_peer_id}) attempting to connect to good user ({good_peer_id1}) at {local_addr_good1}"
    );
    bad_command_handle
        .send_command(Command::ConnectToPeer {
            app_public_key: app_keypair_good1.public(),
            addresses: vec![local_addr_good1.clone()],
        })
        .await;

    // Wait for connection attempts to resolve (or fail due to bad signature)
    sleep(Duration::from_secs(1)).await;

    let (tx_good_peers, rx_good_peers) = oneshot::channel();
    good_command_handle1
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetConnectedPeers {
                response_sender: tx_good_peers,
            },
        ))
        .await;
    let connected_peers_good1 = rx_good_peers.await.unwrap();
    info!(?connected_peers_good1, "Good user connected peers");

    let (tx_good_peers, rx_good_peers) = oneshot::channel();
    good_command_handle2
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetConnectedPeers {
                response_sender: tx_good_peers,
            },
        ))
        .await;
    let connected_peers_good2 = rx_good_peers.await.unwrap();
    info!(?connected_peers_good2, "Second good user connected peers");

    let (tx_bad_peers, rx_bad_peers) = oneshot::channel();
    bad_command_handle
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetConnectedPeers {
                response_sender: tx_bad_peers,
            },
        ))
        .await;
    let connected_peers_bad = rx_bad_peers.await.unwrap();
    info!(?connected_peers_bad, "Bad user connected peers");

    assert!(
        !connected_peers_good1.contains(&app_keypair_bad.public(),),
        "Good user should NOT be connected to bad user due to invalid signature from bad user."
    );
    assert!(
        connected_peers_good1.contains(&app_keypair_good2.public(),),
        "Good user should be connected to second good user."
    );
    assert!(
        !connected_peers_good2.contains(&app_keypair_bad.public(),),
        "Second good user should NOT be connected to bad user due to invalid signature from bad user."
    );
    assert!(
        connected_peers_good2.contains(&app_keypair_good1.public(),),
        "Good user should be connected to good user."
    );
    assert!(
        !connected_peers_bad.contains(&app_keypair_good1.public(),),
        "Bad user should NOT be connected to good user (or anyone else via this connection attempt)."
    );
    assert!(
        !connected_peers_bad.contains(&app_keypair_good2.public(),),
        "Bad user should NOT be connected to second good user (or anyone else via this connection attempt)."
    );

    cancel_good1.cancel();
    cancel_bad.cancel();
    tasks.close();
}
