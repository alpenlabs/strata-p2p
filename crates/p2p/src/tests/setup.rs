//! Tests for the setup phase of P2P connections.

use std::{collections::HashSet, time::Duration};

use libp2p::{Multiaddr, build_multiaddr, identity::Keypair};
use tokio::time::sleep;
use tracing::info;

use crate::{
    commands::{Command, QueryP2PStateCommand}, swarm::filtering::AllowList, tests::common::{init_tracing, Setup, User}
};

/// Test that get_app_public_key returns the correct key after setup phase.
#[tokio::test]
async fn test_get_app_public_key_command() {
    init_tracing();

    info!("Starting get_app_public_key command test");

    // Create a setup with 2 users
    let setup = Setup::all_to_all(2).await.unwrap();

    // Give some time for the setup phase to complete
    sleep(Duration::from_secs(2)).await;

    let user1 = &setup.user_handles[0];
    let user2 = &setup.user_handles[1];

    // Get connected peers
    let user1_peers = user1.command.get_connected_peers().await;
    let user2_peers = user2.command.get_connected_peers().await;

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
    let app_key_from_user1 = user1.command.get_app_public_key(user2_peer_id).await;

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
    let app_key_from_user2 = user2.command.get_app_public_key(user1_peer_id).await;

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
    let non_existent_key = user1.command.get_app_public_key(fake_peer_id).await;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_allowlist_and_banlist() -> anyhow::Result<()> {
    init_tracing();
    info!("Starting allowlist and banlist test");

    // 1. Create two users via all_to-all(2). They will have empty banlist by default.
    let Setup {
        user_handles: original_user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(2).await?;

    let user1_handle = &original_user_handles[0];
    let user2_handle = &original_user_handles[1];

    let user1_app_pk = user1_handle.app_keypair.public();
    let user2_app_pk = user2_handle.app_keypair.public();
    let user1_peer_id = user1_handle.peer_id;
    let user2_peer_id = user2_handle.peer_id;

    info!(%user1_peer_id, ?user1_app_pk, "User1");
    info!(%user2_peer_id, ?user2_app_pk, "User2");

    // Wait for initial all-to-all connections to establish
    sleep(Duration::from_secs(2)).await;

    // Verify user1 and user2 are connected to each other
    let user1_connected_peers = user1_handle.command.get_connected_peers().await;
    assert_eq!(
        user1_connected_peers.len(),
        1,
        "User1 should be connected to 1 peer after all_to_all setup"
    );
    assert!(
        user1_connected_peers.contains(&user2_peer_id),
        "User1 should be connected to User2"
    );

    let user2_connected_peers = user2_handle.command.get_connected_peers().await;
    assert_eq!(
        user2_connected_peers.len(),
        1,
        "User2 should be connected to 1 peer after all_to_all setup"
    );
    assert!(
        user2_connected_peers.contains(&user1_peer_id),
        "User2 should be connected to User1"
    );

    // Get listening addresses for the original users to allow new users to connect
    let mut original_user_addrs = Vec::new();
    let (tx1, rx1) = tokio::sync::oneshot::channel::<Vec<Multiaddr>>();
    user1_handle
        .command
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetMyListeningAddresses {
                response_sender: tx1,
            },
        ))
        .await;
    original_user_addrs.push(rx1.await.unwrap()[0].clone());

    let (tx2, rx2) = tokio::sync::oneshot::channel::<Vec<Multiaddr>>();
    user2_handle
        .command
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetMyListeningAddresses {
                response_sender: tx2,
            },
        ))
        .await;
    original_user_addrs.push(rx2.await.unwrap()[0].clone());

    let user1_addr = original_user_addrs[0].clone();
    let user2_addr = original_user_addrs[1].clone();

    info!("User1 listening addr: {}", user1_addr);
    info!("User2 listening addr: {}", user2_addr);

    // 2. Create another two users manually calling User::new(...) new_user1: AllowList with
    //    user2_app_pk (should only connect to user2) new_user2: AllowList with user1_app_pk (should
    //    only connect to user1)

    // New User 1 setup
    let new_user1_app_keypair = Keypair::generate_ed25519();
    let new_user1_transport_keypair = Keypair::generate_ed25519();
    let new_user1_local_addr = build_multiaddr!(Memory(90000001_u64));
    let mut new_user1_allowlist_set = HashSet::new();
    new_user1_allowlist_set.insert(user2_app_pk.clone()); // Only allow user2
    let new_user1_allowlist = AllowList::new(new_user1_allowlist_set);

    let mut new_user1 = User::new(
        new_user1_app_keypair.clone(),
        new_user1_transport_keypair.clone(),
        vec![user1_addr.clone(), user2_addr.clone()], // Tries to connect to both old users
        new_user1_local_addr.clone(),
        new_user1_allowlist,
        cancel.child_token(),
    )?;
    let new_user1_peer_id = new_user1_transport_keypair.public().to_peer_id();
    let new_user1_handle = new_user1.command.clone(); // Capture handle for later queries

    let new_user1_public_key = new_user1_app_keypair.public();
    info!(
        %new_user1_peer_id, ?new_user1_public_key, "NewUser1",
    );

    // New User 2 setup
    let new_user2_app_keypair = Keypair::generate_ed25519();
    let new_user2_transport_keypair = Keypair::generate_ed25519();
    let new_user2_local_addr = build_multiaddr!(Memory(90000002_u64));
    let mut new_user2_allowlist_set = HashSet::new();
    new_user2_allowlist_set.insert(user1_app_pk.clone()); // Only allow user1
    let new_user2_allowlist = AllowList::new(new_user2_allowlist_set);

    let mut new_user2 = User::new(
        new_user2_app_keypair.clone(),
        new_user2_transport_keypair.clone(),
        vec![user1_addr.clone(), user2_addr.clone()], // Tries to connect to both old users
        new_user2_local_addr.clone(),
        new_user2_allowlist,
        cancel.child_token(),
    )?;
    let new_user2_peer_id = new_user2_transport_keypair.public().to_peer_id();
    let new_user2_handle = new_user2.command.clone(); // Capture handle for later queries

    let new_user2_public_key = new_user2_app_keypair.public();
    info!(
        %new_user2_peer_id, ?new_user2_public_key, "NewUser2",
    );

    // Spawn new users to listen and establish connections
    tasks.spawn(async move {
        info!("NewUser1 establishing connections");
        new_user1.p2p.establish_connections().await;
        info!("NewUser1 connections established. Starting listen.");
        new_user1.p2p.listen().await;
    });

    tasks.spawn(async move {
        info!("NewUser2 establishing connections");
        new_user2.p2p.establish_connections().await;
        info!("NewUser2 connections established. Starting listen.");
        new_user2.p2p.listen().await;
    });

    // Give time for connections to stabilize
    sleep(Duration::from_secs(5)).await;

    // Verify connections for NewUser1 (should only be connected to User2)
    let new_user1_final_peers = new_user1_handle.get_connected_peers().await;
    info!("NewUser1 connected peers: {:?}", new_user1_final_peers);
    assert_eq!(
        new_user1_final_peers.len(),
        1,
        "NewUser1 should be connected to exactly 1 peer"
    );
    assert!(
        new_user1_final_peers.contains(&user2_peer_id),
        "NewUser1 should be connected to User2"
    );
    assert!(
        !new_user1_final_peers.contains(&user1_peer_id),
        "NewUser1 should NOT be connected to User1 due to AllowList"
    );

    // Verify connections for NewUser2 (should only be connected to User1)
    let new_user2_final_peers = new_user2_handle.get_connected_peers().await;
    info!("NewUser2 connected peers: {:?}", new_user2_final_peers);
    assert_eq!(
        new_user2_final_peers.len(),
        1,
        "NewUser2 should be connected to exactly 1 peer"
    );
    assert!(
        new_user2_final_peers.contains(&user1_peer_id),
        "NewUser2 should be connected to User1"
    );
    assert!(
        !new_user2_final_peers.contains(&user2_peer_id),
        "NewUser2 should NOT be connected to User2 due to AllowList"
    );

    // Also verify what original users see.
    // User1 should be connected to User2 (from initial setup) and NewUser2 (because NewUser2
    // connected to User1 and User1 accepted).
    let user1_final_peers = user1_handle.command.get_connected_peers().await;
    info!("User1 final connected peers: {:?}", user1_final_peers);
    assert_eq!(
        user1_final_peers.len(),
        2,
        "User1 should be connected to 2 peers"
    );
    assert!(
        user1_final_peers.contains(&user2_peer_id),
        "User1 should still be connected to User2"
    );
    assert!(
        user1_final_peers.contains(&new_user2_peer_id),
        "User1 should be connected to NewUser2"
    );
    assert!(
        !user1_final_peers.contains(&new_user1_peer_id),
        "User1 should NOT be connected to NewUser1 (NewUser1 rejected it)"
    );

    // User2 should be connected to User1 (from initial setup) and NewUser1 (because NewUser1
    // connected to User2 and User2 accepted).
    let user2_final_peers = user2_handle.command.get_connected_peers().await;
    info!("User2 final connected peers: {:?}", user2_final_peers);
    assert_eq!(
        user2_final_peers.len(),
        2,
        "User2 should be connected to 2 peers"
    );
    assert!(
        user2_final_peers.contains(&user1_peer_id),
        "User2 should still be connected to User1"
    );
    assert!(
        user2_final_peers.contains(&new_user1_peer_id),
        "User2 should be connected to NewUser1"
    );
    assert!(
        !user2_final_peers.contains(&new_user2_peer_id),
        "User2 should NOT be connected to NewUser2 (NewUser2 rejected it)"
    );

    info!("Allowlist and banlist test completed successfully");

    // Clean up
    cancel.cancel();
    tasks.wait().await;
    Ok(())
}
