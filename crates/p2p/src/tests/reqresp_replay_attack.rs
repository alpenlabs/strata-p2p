//! Tests for replay attack prevention.

#![cfg(all(feature = "byos", feature = "request-response"))]

use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use libp2p::{build_multiaddr, connection_limits::ConnectionLimits, identity::Keypair};
use tokio::time::{sleep, timeout};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::info;

#[cfg(feature = "mem-conn-limits-abs")]
use crate::tests::common::SIXTEEN_GIBIBYTES;
use crate::{
    events::ReqRespEvent,
    swarm::message::{
        request_response::{RequestMessage, SignedRequestMessage},
        signed::SignedMessage,
    },
    tests::common::{MockApplicationSigner, User, init_tracing},
};

/// This test verifies that a signed request message intended for one peer cannot be
/// replayed to another peer.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_reqresp_replay_attack_prevention() -> anyhow::Result<()> {
    init_tracing();

    let tasks = TaskTracker::new();

    // Create keypairs for Alice (Sender), Bob (Intended Recipient), and Charlie (Victim)
    let alice_app_keypair = Keypair::generate_ed25519();
    let bob_app_keypair = Keypair::generate_ed25519();
    let charlie_app_keypair = Keypair::generate_ed25519();

    let alice_transport_keypair = Keypair::generate_ed25519();
    let bob_transport_keypair = Keypair::generate_ed25519();
    let charlie_transport_keypair = Keypair::generate_ed25519();

    let alice_addr = build_multiaddr!(Memory(rand::random::<u64>()));
    let bob_addr = build_multiaddr!(Memory(rand::random::<u64>()));
    let charlie_addr = build_multiaddr!(Memory(rand::random::<u64>()));

    let cancel_alice = CancellationToken::new();
    let cancel_bob = CancellationToken::new();
    let cancel_charlie = CancellationToken::new();

    // All users know each other (allowlist)
    let allowlist = vec![
        alice_app_keypair.public(),
        bob_app_keypair.public(),
        charlie_app_keypair.public(),
    ];

    // Alice
    let mut alice_user = User::new(
        alice_app_keypair.clone(),
        alice_transport_keypair.clone(),
        vec![bob_addr.clone(), charlie_addr.clone()],
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

    // Bob (Intended Recipient) - we need his PeerId
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

    // Charlie (Victim)
    let mut charlie_user = User::new(
        charlie_app_keypair.clone(),
        charlie_transport_keypair.clone(),
        vec![alice_addr.clone()],
        allowlist.clone(),
        vec![charlie_addr.clone()],
        cancel_charlie.child_token(),
        Arc::new(MockApplicationSigner::new(charlie_app_keypair.clone())),
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

    let charlie_peer_id = charlie_user.p2p.local_peer_id();
    let bob_peer_id = bob_user.p2p.local_peer_id();
    let mut charlie_reqresp = charlie_user.reqresp;

    tasks.spawn(async move {
        alice_user.p2p.establish_connections().await;
        alice_user.p2p.listen().await;
    });

    tasks.spawn(async move {
        bob_user.p2p.establish_connections().await;
        bob_user.p2p.listen().await;
    });

    tasks.spawn(async move {
        charlie_user.p2p.establish_connections().await;
        charlie_user.p2p.listen().await;
    });

    // Wait for setup to complete
    sleep(Duration::from_secs(2)).await;

    info!("Setup complete. Alice, Bob, and Charlie are connected.");

    // --- ATTACK SIMULATION ---

    // 1. Alice constructs a valid request message intended for Bob
    let request_payload = b"Secret message for Bob".to_vec();

    // Note: RequestMessage::new takes destination_peer_id
    let request_msg = RequestMessage::new(
        alice_app_keypair.public(),
        request_payload.clone(),
        bob_peer_id, // Intended for Bob
    );

    // 2. Sign it with Alice's key
    let signer = MockApplicationSigner::new(alice_app_keypair.clone());
    let signed_msg: SignedRequestMessage = SignedMessage::new(request_msg, &signer)
        .await
        .expect("Failed to sign message");

    // 3. Serialize it (this is what goes over the wire)
    let wire_bytes = flexbuffers::to_vec(&signed_msg).expect("Failed to serialize");

    info!("Alice sending replayed message (intended for Bob) to Charlie...");

    // 4. Spawn Attacker node to replay message
    let attacker_transport_keypair = Keypair::generate_ed25519();
    let attacker_addr = build_multiaddr!(Memory(rand::random::<u64>()));
    let cancel_attacker = CancellationToken::new();

    let attacker_app_keypair = Keypair::generate_ed25519(); // Just some key
    let mut attacker_user = User::new(
        attacker_app_keypair.clone(),
        attacker_transport_keypair,
        vec![charlie_addr.clone()],
        allowlist.clone(), // Attacker knows the allowlist
        vec![attacker_addr],
        cancel_attacker.child_token(),
        Arc::new(MockApplicationSigner::new(attacker_app_keypair)),
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

    // Connect attacker to Charlie
    attacker_user.p2p.establish_connections().await;

    // Drive attacker swarm until connected to Charlie
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > Duration::from_secs(5) {
            panic!("Attacker failed to connect to Charlie");
        }

        tokio::select! {
            _ = attacker_user.p2p.swarm_mut().select_next_some() => {
                // Swarm made progress
                if attacker_user.p2p.is_connected(&charlie_peer_id) {
                     info!("Attacker connected to Charlie");
                     break;
                }
            }
            _ = sleep(Duration::from_millis(10)) => {}
        }
    }

    // Now we are connected. Send the malicious request.
    info!("Injecting replayed message...");

    attacker_user
        .p2p
        .swarm_mut()
        .behaviour_mut()
        .request_response
        .send_request(&charlie_peer_id, wire_bytes);

    // Continue driving attacker swarm for a bit to ensure message is flushed
    let _ = timeout(Duration::from_secs(1), async {
        loop {
            attacker_user.p2p.swarm_mut().select_next_some().await;
        }
    })
    .await;

    // Now verify Charlie received nothing
    info!("Checking Charlie's inbox...");

    let result = timeout(Duration::from_secs(2), charlie_reqresp.next()).await;

    match result {
        Ok(Some(ReqRespEvent::ReceivedRequest(data, _))) => {
            // If we receive a request, it means the attack succeeded (BAD)
            panic!(
                "Security breach! Charlie accepted replayed message intended for Bob. Data: {:?}",
                data
            );
        }
        Ok(Some(event)) => {
            info!("Charlie received unrelated event: {:?}", event);
        }
        Ok(None) => {
            info!("Charlie stream ended.");
        }
        Err(_) => {
            info!("Success! Charlie did not receive the replayed message (timeout).");
        }
    }

    cancel_alice.cancel();
    cancel_bob.cancel();
    cancel_charlie.cancel();
    cancel_attacker.cancel();
    tasks.close();
    tasks.wait().await;

    Ok(())
}
