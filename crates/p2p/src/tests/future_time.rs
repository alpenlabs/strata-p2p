#![cfg(all(feature = "gossipsub", feature = "byos"))]

use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use libp2p::{connection_limits::ConnectionLimits, gossipsub::Sha256Topic, swarm::SwarmEvent};
use tokio::{
    pin, select,
    time::{Instant, sleep},
};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

#[cfg(feature = "mem-conn-limits-abs")]
use crate::tests::common::SIXTEEN_GIBIBYTES;
use crate::{
    events::GossipEvent,
    swarm::{
        behavior::BehaviourEvent,
        message::{
            ProtocolId, get_timestamp,
            gossipsub::{GossipMessage, SignedGossipsubMessage},
        },
        setup::events::SetupBehaviourEvent,
    },
    tests::common::{MockApplicationSigner, Setup, SetupInitialData, User, init_tracing},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_gossip_future_time() -> anyhow::Result<()> {
    init_tracing();

    // 1. Setup Data for 2 nodes
    let SetupInitialData {
        app_keypairs,
        transport_keypairs,
        peer_ids: _peer_ids,
        multiaddresses,
    } = Setup::setup_keys_ids_addrs_of_n_users(2);

    let victim_idx = 0;
    let attacker_idx = 1;

    let victim_app_kp = app_keypairs[victim_idx].clone();
    let victim_transport_kp = transport_keypairs[victim_idx].clone();
    let victim_addr = multiaddresses[victim_idx].clone();

    let attacker_app_kp = app_keypairs[attacker_idx].clone();
    let attacker_transport_kp = transport_keypairs[attacker_idx].clone();
    let attacker_addr = multiaddresses[attacker_idx].clone();

    // 2. Setup Nodes
    let cancel = CancellationToken::new();

    // Allow 1 second skew
    let skew_duration = Duration::from_secs(1);
    let max_clock_skew = Some(skew_duration);
    let envelope_max_age = Some(Duration::from_secs(60));

    let victim_user = User::new_with_timeouts(
        victim_app_kp.clone(),
        victim_transport_kp.clone(),
        vec![],                         // Connect to no one initially
        vec![attacker_app_kp.public()], // Allow attacker
        vec![victim_addr.clone()],
        cancel.child_token(),
        Arc::new(MockApplicationSigner::new(victim_app_kp.clone())),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
        envelope_max_age,
        max_clock_skew,
    )?;

    let mut attacker_user = User::new_with_timeouts(
        attacker_app_kp.clone(),
        attacker_transport_kp.clone(),
        vec![victim_addr.clone()],    // Connect to victim
        vec![victim_app_kp.public()], // Allow victim
        vec![attacker_addr.clone()],
        cancel.child_token(),
        Arc::new(MockApplicationSigner::new(attacker_app_kp.clone())),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
        envelope_max_age,
        max_clock_skew,
    )?;

    let (mut handles, _tasks) = Setup::start_users(vec![victim_user]).await;
    let mut victim = handles.pop().unwrap();

    // Drive attacker manually
    let attacker_swarm = attacker_user.p2p.swarm_mut();
    attacker_swarm.dial(victim_addr.clone())?;

    // Wait for connection and Setup handshake
    let mut connected = false;
    let start_connect = Instant::now();
    loop {
        if start_connect.elapsed() > Duration::from_secs(5) {
            break;
        }
        select! {
            event = attacker_swarm.select_next_some() => {
                if let SwarmEvent::Behaviour(BehaviourEvent::Setup(SetupBehaviourEvent::AppKeyReceived { .. })) = event {
                    info!("Attacker finished setup with Victim");
                    connected = true;
                }
            },
            _ = sleep(Duration::from_millis(100)) => {}
        }
        if connected {
            break;
        }
    }
    assert!(connected, "Failed to connect and setup");

    // Subscribe to topic
    let topic = Sha256Topic::new("strata");
    attacker_swarm.behaviour_mut().gossipsub.subscribe(&topic)?;

    // Wait for mesh formation
    let start_wait = Instant::now();
    let mut mesh_formed = false;
    loop {
        if start_wait.elapsed() > Duration::from_secs(5) {
            warn!("Timeout waiting for mesh formation");
            break;
        }
        select! {
            _ = attacker_swarm.select_next_some() => {},
            _ = sleep(Duration::from_millis(100)) => {}
        }

        if attacker_swarm
            .behaviour()
            .gossipsub
            .mesh_peers(&topic.hash())
            .count()
            > 0
        {
            info!("Mesh formed with victim");
            mesh_formed = true;
            break;
        }
    }
    assert!(mesh_formed, "Mesh not formed");

    // 5. Construct Malicious Message (Future Date)
    let future_time = get_timestamp() + 10 + skew_duration.as_secs();

    let gossip_msg = GossipMessage {
        version: Default::default(),
        protocol: ProtocolId::Gossip,
        message: b"message from future".to_vec(),
        public_key: attacker_app_kp.public(),
        date: future_time,
    };

    let serialized_msg = flexbuffers::to_vec(&gossip_msg).unwrap();
    let signature = attacker_app_kp.sign(&serialized_msg).unwrap();
    let signature_array: [u8; 64] = signature.try_into().unwrap();

    let signed_msg = SignedGossipsubMessage {
        message: gossip_msg,
        signature: signature_array,
    };

    let payload = flexbuffers::to_vec(&signed_msg).unwrap();

    // 6. Publish Malicious Message
    match attacker_swarm
        .behaviour_mut()
        .gossipsub
        .publish(topic.clone(), payload)
    {
        Ok(_) => info!("Attacker published future message"),
        Err(e) => panic!("Failed to publish: {:?}", e),
    }

    // 7. Verify Victim does NOT receive it
    let timeout = sleep(Duration::from_secs(2));
    pin!(timeout);

    loop {
        select! {
            _ = &mut timeout => {
                info!("Timeout reached, no message received (Expected)");
                break;
            }
            event = victim.gossip.next_event() => {
                if let Ok(GossipEvent::ReceivedMessage(data)) = event && data == b"message from future" {
                    panic!("Victim received future message! Test Failed.");
                }
            }
            _ = attacker_swarm.select_next_some() => {}
        }
    }

    // 8. Verify Positive Control: Publish Valid Message
    let valid_msg_content = b"valid message";
    let valid_time = get_timestamp();

    let gossip_msg_valid = GossipMessage {
        version: Default::default(),
        protocol: ProtocolId::Gossip,
        message: valid_msg_content.to_vec(),
        public_key: attacker_app_kp.public(),
        date: valid_time,
    };

    let serialized_valid = flexbuffers::to_vec(&gossip_msg_valid).unwrap();
    let sig_valid = attacker_app_kp
        .sign(&serialized_valid)
        .unwrap()
        .try_into()
        .unwrap();

    let signed_valid = SignedGossipsubMessage {
        message: gossip_msg_valid,
        signature: sig_valid,
    };

    let payload_valid = flexbuffers::to_vec(&signed_valid).unwrap();

    attacker_swarm
        .behaviour_mut()
        .gossipsub
        .publish(topic, payload_valid)?;
    info!("Attacker published valid message");

    // Expect receipt
    let timeout_valid = sleep(Duration::from_secs(5));
    pin!(timeout_valid);

    let mut received = false;
    loop {
        select! {
            _ = &mut timeout_valid => {
                break;
            }
            event = victim.gossip.next_event() => {
                if let Ok(GossipEvent::ReceivedMessage(data)) = event && data == valid_msg_content {
                    info!("Victim received valid message (Confirmed)");
                    received = true;
                    break;
                }
            }
            _ = attacker_swarm.select_next_some() => {}
        }
    }

    assert!(received, "Victim should have received the valid message");

    cancel.cancel();
    Ok(())
}
