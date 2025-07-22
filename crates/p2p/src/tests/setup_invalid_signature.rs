//! Tests for the setup phase of P2P connections where signature is invalid.

use std::time::Duration;

use libp2p::{
    PeerId, build_multiaddr,
    identity::{Keypair, PublicKey},
};
use tokio::{sync::oneshot::channel, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::{
    commands::{Command, ConnectToPeerCommand, QueryP2PStateCommand},
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
    fn sign(
        &self,
        _message: &[u8],
        _app_public_key: PublicKey,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let signature = Vec::from([0x02; 32]);
        Ok(signature)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_setup_with_invalid_signature() {
    init_tracing();
    let tasks = tokio_util::task::TaskTracker::new();

    // --- Setup Good User ---
    let app_keypair_good = Keypair::generate_ed25519();
    let transport_keypair_good = Keypair::generate_ed25519();
    let local_addr_good = build_multiaddr!(Memory(rand::random::<u64>()));
    let cancel_good = CancellationToken::new();

    let good_user = User::new(
        app_keypair_good.clone(),
        transport_keypair_good.clone(),
        vec![], // No initial connections
        local_addr_good.clone(),
        vec![], // No initial allowlist
        cancel_good.child_token(),
        MockApplicationSigner::new(app_keypair_good.clone()),
    )
    .unwrap();

    let good_peer_id = good_user.transport_keypair.public().to_peer_id();
    let good_command_handle = good_user.command.clone();

    // Spawn good user's P2P listener in a task
    tasks.spawn(async move {
        good_user.p2p.listen().await;
    });

    // --- Setup Bad User ---
    let app_keypair_bad = Keypair::generate_ed25519();
    let transport_keypair_bad = Keypair::generate_ed25519();
    let local_addr_bad = build_multiaddr!(Memory(rand::random::<u64>()));
    let cancel_bad = CancellationToken::new();

    let bad_user = User::new(
        app_keypair_bad.clone(),
        transport_keypair_bad.clone(),
        vec![], // No initial connections
        local_addr_bad.clone(),
        vec![], // No initial allowlist
        cancel_bad.child_token(),
        BadApplicationSigner::new(app_keypair_bad.clone()),
    )
    .unwrap();

    let bad_peer_id = bad_user.transport_keypair.public().to_peer_id();
    let bad_command_handle = bad_user.command.clone();

    // Spawn bad user's P2P listener in a task
    tasks.spawn(async move {
        bad_user.p2p.listen().await;
    });

    // Give some time for listeners to start
    sleep(Duration::from_millis(100)).await;

    // --- Initiate Connections ---
    // Good user tries to connect to bad user
    info!(
        "Good user ({}) attempting to connect to bad user ({}) at {}",
        good_peer_id, bad_peer_id, local_addr_bad
    );
    good_command_handle
        .send_command(Command::ConnectToPeer(ConnectToPeerCommand {
            peer_id: bad_peer_id,
            peer_addr: local_addr_bad.clone(),
        }))
        .await;

    // Bad user tries to connect to good user
    info!(
        "Bad user ({}) attempting to connect to good user ({}) at {}",
        bad_peer_id, good_peer_id, local_addr_good
    );
    bad_command_handle
        .send_command(Command::ConnectToPeer(ConnectToPeerCommand {
            peer_id: good_peer_id,
            peer_addr: local_addr_good.clone(),
        }))
        .await;

    // Wait for connection attempts to resolve (or fail due to bad signature)
    sleep(Duration::from_secs(2)).await;

    // --- Verify Connection States ---
    // Check good user's connections
    let (tx_good_peers, rx_good_peers) = channel::<Vec<PeerId>>();
    good_command_handle
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetConnectedPeers {
                response_sender: tx_good_peers,
            },
        ))
        .await;
    let connected_peers_good = rx_good_peers.await.unwrap();
    info!(?connected_peers_good, "Good user connected peers");

    // Check bad user's connections
    let (tx_bad_peers, rx_bad_peers) = channel::<Vec<PeerId>>();
    bad_command_handle
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetConnectedPeers {
                response_sender: tx_bad_peers,
            },
        ))
        .await;
    let connected_peers_bad = rx_bad_peers.await.unwrap();
    info!(?connected_peers_bad, "Bad user connected peers");

    // Assertions: Neither user should be connected to the other
    assert!(
        !connected_peers_good.contains(&bad_peer_id),
        "Good user should NOT be connected to bad user due to invalid signature from bad user."
    );
    assert!(
        !connected_peers_bad.contains(&good_peer_id),
        "Bad user should NOT be connected to good user (or anyone else via this connection attempt)."
    );

    // --- Cleanup ---
    cancel_good.cancel();
    cancel_bad.cancel();
    tasks.close();
}
