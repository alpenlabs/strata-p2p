//! Tests for the setup phase of P2P connections.

use std::time::Duration;

use tokio::time::sleep;
use tracing::info;

use crate::tests::common::{Setup, init_tracing};

/// Test that peers can connect and are identified by their app public keys after setup phase.
#[tokio::test]
async fn test_connection_by_app_public_key() {
    init_tracing();

    info!("Starting connection by app public key test");

    let setup = Setup::all_to_all(2).await.unwrap();

    sleep(Duration::from_secs(2)).await;

    let user1 = &setup.user_handles[0];
    let user2 = &setup.user_handles[1];

    let user1_peers = user1.command.get_connected_peers(None).await;
    let user2_peers = user2.command.get_connected_peers(None).await;

    assert!(
        !user1_peers.is_empty(),
        "User 1 should have connected peers"
    );
    assert!(
        !user2_peers.is_empty(),
        "User 2 should have connected peers"
    );

    let user2_app_pk = user2.app_keypair.public();
    let is_connected_1_to_2 = user1.command.is_connected(&user2_app_pk, None).await;
    assert!(
        is_connected_1_to_2,
        "User 1 should be connected to user 2 by app public key"
    );

    let user1_app_pk = user1.app_keypair.public();
    let is_connected_2_to_1 = user2.command.is_connected(&user1_app_pk, None).await;
    assert!(
        is_connected_2_to_1,
        "User 2 should be connected to user 1 by app public key"
    );

    let fake_keypair = libp2p::identity::Keypair::generate_ed25519();
    let fake_app_pk = fake_keypair.public();
    let is_connected_to_fake = user1.command.is_connected(&fake_app_pk, None).await;
    assert!(
        !is_connected_to_fake,
        "Should not be connected to non-existent app public key"
    );

    setup.cancel.cancel();
    setup.tasks.wait().await;
}
