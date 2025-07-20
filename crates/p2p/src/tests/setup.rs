//! Tests for the setup phase of P2P connections.

use std::time::Duration;

use tokio::time::sleep;
use tracing::info;

use crate::tests::common::Setup;

/// Test that get_app_public_key returns the correct key after setup phase.
#[tokio::test]
async fn test_get_app_public_key_command() {
    // Initialize tracing for this test if not already done
    let _ = tracing_subscriber::fmt::try_init();

    info!("Starting get_app_public_key command test");

    // Create a setup with 2 users
    let setup = Setup::all_to_all(2).await.unwrap();

    // Give some time for the setup phase to complete
    sleep(Duration::from_secs(2)).await;

    let user1 = &setup.user_handles[0];
    let user2 = &setup.user_handles[1];

    // Get connected peers
    let user1_peers = user1.handle.get_connected_peers().await;
    let user2_peers = user2.handle.get_connected_peers().await;

    assert!(
        !user1_peers.is_empty(),
        "User 1 should have connected peers"
    );
    assert!(
        !user2_peers.is_empty(),
        "User 2 should have connected peers"
    );

    // Test getting app public key for user2 from user1's perspective
    let user2_peer_id = user2.peer_id;
    let app_key_from_user1 = user1.handle.get_app_public_key(user2_peer_id).await;

    // Should have the app public key after setup
    assert!(
        app_key_from_user1.is_some(),
        "User 1 should have user 2's app public key"
    );

    // The retrieved key should match user2's actual app public key
    assert_eq!(
        app_key_from_user1.unwrap(),
        user2.app_keypair.public(),
        "Retrieved app public key should match user 2's actual app public key"
    );

    // Test getting app public key for user1 from user2's perspective
    let user1_peer_id = user1.peer_id;
    let app_key_from_user2 = user2.handle.get_app_public_key(user1_peer_id).await;

    // Should have the app public key after setup
    assert!(
        app_key_from_user2.is_some(),
        "User 2 should have user 1's app public key"
    );

    // The retrieved key should match user1's actual app public key
    assert_eq!(
        app_key_from_user2.unwrap(),
        user1.app_keypair.public(),
        "Retrieved app public key should match user 1's actual app public key"
    );

    // Test getting app public key for a non-existent peer
    let fake_keypair = libp2p::identity::Keypair::generate_ed25519();
    let fake_peer_id = fake_keypair.public().to_peer_id();
    let non_existent_key = user1.handle.get_app_public_key(fake_peer_id).await;

    // Should return None for non-connected peer
    assert!(
        non_existent_key.is_none(),
        "Should return None for non-connected peer"
    );

    info!("get_app_public_key command test completed successfully");

    // Clean up
    setup.cancel.cancel();
    setup.tasks.wait().await;
}
