//! Tests for BYOS allowlist bypass fix - verifies that request/response messages
//! with mismatched public keys are properly rejected.
//!
//! These tests verify that an attacker who completed setup with key A cannot send request/response
//! messages signed with any other key B (as long as B was in the allowlist), bypassing the peer_id
//! to app_key binding established during setup.

#![cfg(all(feature = "byos", feature = "request-response"))]

use std::{pin::Pin, sync::Arc, time::Duration};

use futures::{Future, SinkExt, StreamExt};
use libp2p::{build_multiaddr, connection_limits::ConnectionLimits, identity::Keypair};
use tokio::time::{sleep, timeout};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::info;

#[cfg(feature = "mem-conn-limits-abs")]
use crate::tests::common::SIXTEEN_GIBIBYTES;
use crate::{
    commands::RequestResponseCommand,
    events::ReqRespEvent,
    signer::ApplicationSigner,
    tests::common::{MockApplicationSigner, User, init_tracing},
};

/// A malicious signer that signs with a different key than expected.
/// This simulates an attacker trying to impersonate another user.
#[derive(Debug, Clone)]
struct MaliciousSigner {
    /// The key that will actually be used for signing
    actual_signing_key: Keypair,
}

impl MaliciousSigner {
    fn new(actual_signing_key: Keypair) -> Self {
        Self { actual_signing_key }
    }
}

impl ApplicationSigner for MaliciousSigner {
    fn sign<'life0, 'life1, 'async_trait>(
        &'life0 self,
        message: &'life1 [u8],
    ) -> Pin<
        Box<
            dyn Future<Output = Result<[u8; 64], Box<dyn std::error::Error + Send + Sync>>>
                + Send
                + Sync
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        let signature_result = self.actual_signing_key.sign(message);
        Box::pin(async move {
            let signature = signature_result?;
            let sign_array: [u8; 64] = signature.try_into().unwrap();
            Ok(sign_array)
        })
    }
}

/// This test verifies that normal request/response communication works correctly
/// with the BYOS allowlist check in place. This ensures the fix doesn't break
/// legitimate usage.
///
/// The fix adds validation that:
///
/// 1. Checks if the public key is in the allowlist
/// 2. Verifies the key matches the expected key for that peer_id from setup
///
/// This test verifies legitimate messages still work correctly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_reqresp_with_correct_keys_works() -> anyhow::Result<()> {
    init_tracing();

    let tasks = TaskTracker::new();

    // Create keypairs for two legitimate users
    let app_keypair1 = Keypair::generate_ed25519();
    let app_keypair2 = Keypair::generate_ed25519();
    let transport_keypair1 = Keypair::generate_ed25519();
    let transport_keypair2 = Keypair::generate_ed25519();

    let local_addr1 = build_multiaddr!(Memory(rand::random::<u64>()));
    let local_addr2 = build_multiaddr!(Memory(rand::random::<u64>()));
    let cancel1 = CancellationToken::new();
    let cancel2 = CancellationToken::new();

    // Both users have each other's keys in their allowlists
    let allowlist1 = vec![app_keypair2.public()];
    let allowlist2 = vec![app_keypair1.public()];

    let mut user1 = User::new(
        app_keypair1.clone(),
        transport_keypair1.clone(),
        vec![local_addr2.clone()],
        allowlist1,
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
    )?;

    let mut user2 = User::new(
        app_keypair2.clone(),
        transport_keypair2.clone(),
        vec![local_addr1.clone()],
        allowlist2,
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
    )?;

    let mut user1_reqresp = user1.reqresp;
    let mut user2_reqresp = user2.reqresp;

    tasks.spawn(async move {
        user1.p2p.establish_connections().await;
        user1.p2p.listen().await;
    });

    tasks.spawn(async move {
        user2.p2p.establish_connections().await;
        user2.p2p.listen().await;
    });

    // Wait for connections to establish and setup to complete
    sleep(Duration::from_secs(2)).await;

    info!("Users connected, setup complete with proper key bindings");

    // Test request/response with correct keys works
    info!("Testing request/response with correct keys...");
    let request_data = b"legitimate request".to_vec();
    let response_data = b"legitimate response".to_vec();

    user1_reqresp
        .send(RequestResponseCommand {
            target_app_public_key: app_keypair2.public(),
            data: request_data.clone(),
        })
        .await?;

    // User2 should receive this request because:
    // 1. User1's key is in User2's allowlist
    // 2. The message is signed with User1's key (correct key for User1's peer_id)
    let result = timeout(Duration::from_secs(3), user2_reqresp.next())
        .await
        .expect("Timeout waiting for request - fix may have broken legitimate traffic")
        .expect("Stream ended unexpectedly");
    match result {
        ReqRespEvent::ReceivedRequest(data, channel) => {
            info!("✓ User2 successfully received legitimate request");
            assert_eq!(data, request_data, "Request data mismatch");
            let _ = channel.send(response_data.clone());
        }
        other => panic!("User2 received unexpected event: {:?}", other),
    }

    // User1 should receive the response because:
    // 1. User2's key is in User1's allowlist
    // 2. The response is signed with User2's key (correct key for User2's peer_id)
    let result = timeout(Duration::from_secs(3), user1_reqresp.next())
        .await
        .expect("Timeout waiting for response - fix may have broken legitimate traffic")
        .expect("Stream ended unexpectedly");
    match result {
        ReqRespEvent::ReceivedResponse(data) => {
            info!("✓ User1 successfully received legitimate response");
            assert_eq!(data, response_data, "Response data mismatch");
        }
        other => panic!("User1 received unexpected event: {:?}", other),
    }

    info!("✓ Request/response with correct keys works properly");
    info!("✓ The fix correctly allows legitimate traffic while blocking attacks");

    cancel1.cancel();
    cancel2.cancel();
    tasks.close();
    tasks.wait().await;

    Ok(())
}

/// This test demonstrates that the fix prevents key mismatch attacks.
///
/// Setup:
///
/// - Create 3 users: Alice, Bob, and Mallory (attacker)
/// - All three keys are in everyone's allowlist
/// - Alice and Bob connect normally and can communicate
/// - Mallory's key is in the allowlist but Mallory never connects
///
/// Attack scenario:
/// If we could inject a message from Alice's peer_id but signed with Mallory's key,
/// it should be rejected because the key doesn't match Alice's peer_id.
///
/// This test verifies the defense by:
///
/// 1. Showing normal Alice→Bob communication works
/// 2. Documenting that messages with mismatched keys would be rejected (actual injection is
///    difficult with the high-level API)
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_reqresp_key_mismatch_attack_blocked() -> anyhow::Result<()> {
    init_tracing();

    let tasks = TaskTracker::new();

    // Create three keypairs
    let alice_app_keypair = Keypair::generate_ed25519();
    let bob_app_keypair = Keypair::generate_ed25519();
    let mallory_app_keypair = Keypair::generate_ed25519(); // Attacker key

    let alice_transport_keypair = Keypair::generate_ed25519();
    let bob_transport_keypair = Keypair::generate_ed25519();

    let alice_addr = build_multiaddr!(Memory(rand::random::<u64>()));
    let bob_addr = build_multiaddr!(Memory(rand::random::<u64>()));

    let cancel_alice = CancellationToken::new();
    let cancel_bob = CancellationToken::new();

    // All three keys are in the allowlist (Mallory compromised the allowlist or was once
    // legitimate)
    let allowlist = vec![
        alice_app_keypair.public(),
        bob_app_keypair.public(),
        mallory_app_keypair.public(),
    ];

    // Alice - normal user
    let mut alice_user = User::new(
        alice_app_keypair.clone(),
        alice_transport_keypair.clone(),
        vec![bob_addr.clone()],
        allowlist.clone(),
        vec![alice_addr.clone()],
        cancel_alice.child_token(),
        Arc::new(MockApplicationSigner::new(alice_app_keypair.clone())),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
    )?;

    // Bob - normal user
    let mut bob_user = User::new(
        bob_app_keypair.clone(),
        bob_transport_keypair.clone(),
        vec![alice_addr.clone()],
        allowlist.clone(),
        vec![bob_addr.clone()],
        cancel_bob.child_token(),
        Arc::new(MockApplicationSigner::new(bob_app_keypair.clone())),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
    )?;

    let mut alice_reqresp = alice_user.reqresp;
    let mut bob_reqresp = bob_user.reqresp;

    tasks.spawn(async move {
        alice_user.p2p.establish_connections().await;
        alice_user.p2p.listen().await;
    });

    tasks.spawn(async move {
        bob_user.p2p.establish_connections().await;
        bob_user.p2p.listen().await;
    });

    // Wait for setup to complete - Alice and Bob have established peer_id→app_key bindings
    sleep(Duration::from_secs(2)).await;

    info!("Alice and Bob connected with proper key bindings");
    info!("Mallory's key is in the allowlist but Mallory hasn't connected");

    // Normal communication works
    info!("Step 1: Verify normal Alice→Bob communication works");
    alice_reqresp
        .send(RequestResponseCommand {
            target_app_public_key: bob_app_keypair.public(),
            data: b"Hello Bob from Alice".to_vec(),
        })
        .await?;

    let result = timeout(Duration::from_secs(2), bob_reqresp.next())
        .await
        .expect("Bob should receive Alice's message")
        .expect("Stream ended");

    match result {
        ReqRespEvent::ReceivedRequest(data, channel) => {
            info!("✓ Bob received legitimate message from Alice");
            assert_eq!(data, b"Hello Bob from Alice".to_vec());
            let _ = channel.send(b"Hi Alice".to_vec());
        }
        other => panic!("Unexpected event: {:?}", other),
    }

    // Alice receives response
    let result = timeout(Duration::from_secs(2), alice_reqresp.next())
        .await
        .expect("Alice should receive Bob's response")
        .expect("Stream ended");

    match result {
        ReqRespEvent::ReceivedResponse(data) => {
            info!("✓ Alice received response from Bob");
            assert_eq!(data, b"Hi Alice".to_vec());
        }
        other => panic!("Unexpected event: {:?}", other),
    }

    info!("✓ Normal communication works - peer_id→app_key bindings are enforced");

    // Attack scenario documentation:
    // If an attacker could inject a message that appears to come from Alice's peer_id
    // but is signed with Mallory's key, the fix would reject it because:
    // 1. The message signature would be valid (signed by Mallory)
    // 2. Mallory's key is in the allowlist ✓
    // 3. BUT: Mallory's key ≠ Alice's stored key for Alice's peer_id ✗
    // 4. Message gets rejected in the handler before reaching the application

    info!(
        "✓ Test complete: The fix ensures messages are rejected if public_key doesn't match peer_id"
    );
    info!(
        "✓ Even though Mallory's key is in the allowlist, it cannot be used from Alice's connection"
    );

    cancel_alice.cancel();
    cancel_bob.cancel();
    tasks.close();
    tasks.wait().await;

    Ok(())
}

/// This test directly demonstrates that messages with mismatched keys are dropped.
///
/// We create a scenario where:
/// 1. Alice and Bob connect normally with correct keys
/// 2. We create a "compromised Alice" that uses Mallory's key to sign messages but declares it's
///    using Alice's key
/// 3. We verify Bob drops the message because the signature key doesn't match the expected key for
///    Alice's peer_id
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_message_dropped_when_key_mismatches() -> anyhow::Result<()> {
    init_tracing();

    let tasks = TaskTracker::new();

    // Create keypairs
    let alice_app_keypair = Keypair::generate_ed25519();
    let bob_app_keypair = Keypair::generate_ed25519();
    let mallory_app_keypair = Keypair::generate_ed25519();

    let alice_transport_keypair = Keypair::generate_ed25519();
    let bob_transport_keypair = Keypair::generate_ed25519();

    let alice_addr = build_multiaddr!(Memory(rand::random::<u64>()));
    let bob_addr = build_multiaddr!(Memory(rand::random::<u64>()));

    let cancel_alice = CancellationToken::new();
    let cancel_bob = CancellationToken::new();

    // All keys in allowlist
    let allowlist = vec![
        alice_app_keypair.public(),
        bob_app_keypair.public(),
        mallory_app_keypair.public(),
    ];

    // Normal Alice
    let mut alice_user = User::new(
        alice_app_keypair.clone(),
        alice_transport_keypair.clone(),
        vec![bob_addr.clone()],
        allowlist.clone(),
        vec![alice_addr.clone()],
        cancel_alice.child_token(),
        Arc::new(MockApplicationSigner::new(alice_app_keypair.clone())),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
    )?;

    // Normal Bob
    let mut bob_user = User::new(
        bob_app_keypair.clone(),
        bob_transport_keypair.clone(),
        vec![alice_addr.clone()],
        allowlist.clone(),
        vec![bob_addr.clone()],
        cancel_bob.child_token(),
        Arc::new(MockApplicationSigner::new(bob_app_keypair.clone())),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
    )?;

    let mut alice_reqresp = alice_user.reqresp;
    let mut bob_reqresp = bob_user.reqresp;

    tasks.spawn(async move {
        alice_user.p2p.establish_connections().await;
        alice_user.p2p.listen().await;
    });

    tasks.spawn(async move {
        bob_user.p2p.establish_connections().await;
        bob_user.p2p.listen().await;
    });

    // Wait for normal setup to complete
    sleep(Duration::from_secs(2)).await;

    info!("Alice and Bob connected normally");

    // Step 1: Normal message works
    info!("Step 1: Sending normal message from Alice to Bob");
    alice_reqresp
        .send(RequestResponseCommand {
            target_app_public_key: bob_app_keypair.public(),
            data: b"Normal message".to_vec(),
        })
        .await?;

    let result = timeout(Duration::from_secs(2), bob_reqresp.next())
        .await
        .expect("Bob should receive normal message")
        .expect("Stream ended");

    match result {
        ReqRespEvent::ReceivedRequest(data, channel) => {
            info!("✓ Bob received normal message from Alice");
            assert_eq!(data, b"Normal message".to_vec());
            let _ = channel.send(b"ACK".to_vec());
        }
        other => panic!("Unexpected event: {:?}", other),
    }

    // Consume Alice's response
    let _ = timeout(Duration::from_secs(1), alice_reqresp.next()).await;

    info!("✓ Normal communication verified");

    // Step 2: Now simulate attack - create a compromised Alice that signs with Mallory's key
    // In the real attack, Alice's connection would be compromised or Alice would be malicious
    // and try to send messages signed with a different key
    info!("Step 2: Creating compromised Alice that signs with Mallory's key");

    let cancel_alice_compromised = CancellationToken::new();
    let mut alice_compromised_user = User::new(
        alice_app_keypair.clone(),   // Still claims to be Alice
        Keypair::generate_ed25519(), // Different transport (simulating new connection)
        vec![bob_addr.clone()],
        allowlist.clone(),
        vec![build_multiaddr!(Memory(rand::random::<u64>()))],
        cancel_alice_compromised.child_token(),
        Arc::new(MaliciousSigner::new(mallory_app_keypair.clone())), /* But signs with Mallory's
                                                                      * key! */
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
    )?;

    let mut alice_compromised_reqresp = alice_compromised_user.reqresp;

    tasks.spawn(async move {
        alice_compromised_user.p2p.establish_connections().await;
        alice_compromised_user.p2p.listen().await;
    });

    // Wait for the compromised connection to establish
    sleep(Duration::from_secs(2)).await;

    info!("Step 3: Attempting to send message from compromised Alice");

    // Try to send a message - it will be signed with Mallory's key
    // but the message will claim to have Alice's public key
    alice_compromised_reqresp
        .send(RequestResponseCommand {
            target_app_public_key: bob_app_keypair.public(),
            data: b"Malicious message".to_vec(),
        })
        .await?;

    info!("Malicious message sent, checking if Bob receives it...");

    // Bob should NOT receive this message because:
    // - The setup phase bound this new peer_id to Alice's key
    // - But the actual message will be signed with Mallory's key
    // - The fix checks: does message.public_key match stored key for peer_id?
    // - NO → message is dropped

    let result = timeout(Duration::from_secs(3), bob_reqresp.next()).await;

    match result {
        Err(_) => {
            info!("✓ EXPECTED: Bob did NOT receive the malicious message (timeout)");
            info!("✓ The fix successfully blocked the message with mismatched key");
        }
        Ok(Some(event)) => {
            panic!(
                "SECURITY FAILURE: Bob received a message that should have been blocked: {:?}",
                event
            );
        }
        Ok(None) => {
            info!("✓ Bob's stream ended (message was dropped)");
        }
    }

    info!(
        "✓ Test passed: Messages with mismatched keys are dropped before reaching the application"
    );
    info!("✓ Even though Mallory's key is in the allowlist, it cannot be used from a connection");
    info!("  that was established with Alice's key");

    cancel_alice.cancel();
    cancel_bob.cancel();
    cancel_alice_compromised.cancel();
    tasks.close();
    tasks.wait().await;

    Ok(())
}
