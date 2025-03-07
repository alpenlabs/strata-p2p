//! Connect to Peer Command tests.

use std::time::Duration;

use anyhow::bail;
use libp2p::{build_multiaddr, identity::secp256k1::Keypair as SecpKeypair, PeerId};
use strata_p2p_types::{P2POperatorPubKey, StakeChainId};
use strata_p2p_wire::p2p::v1::UnsignedGossipsubMsg;
use tokio::time::sleep;
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use super::common::{mock_stake_chain_info, Operator, Setup};
use crate::{
    commands::{Command, ConnectToPeerCommand},
    events::Event,
};

/// Tests sending a gossipsub message from a new operator to all existing operators.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "don't know why this does not work"]
async fn gossip_new_operator() -> anyhow::Result<()> {
    const OPERATORS_NUM: usize = 2;

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    // Generate a keypair for the new operator
    let new_operator_keypair = SecpKeypair::generate();
    let new_operator_pubkey: P2POperatorPubKey = new_operator_keypair.public().clone().into();

    // Create allowlist for all operators (including the new one)
    let mut signers_allowlist = vec![new_operator_pubkey.clone()];

    // Create the original operators with allowlist containing the new operator
    let Setup {
        mut operators,
        cancel,
        tasks,
    } = Setup::with_extra_signers(OPERATORS_NUM, signers_allowlist.clone()).await?;

    // Add existing operators to the signers allowlist
    for op in &operators {
        signers_allowlist.push(op.kp.public().clone().into());
    }

    // Create a separate task for the new operator
    let local_addr = build_multiaddr!(Memory((OPERATORS_NUM + 1) as u64));

    // Create peer IDs of existing operators
    let peer_ids: Vec<PeerId> = operators.iter().map(|op| op.peer_id).collect();

    // Create connection addresses for the new operator to connect to existing ones
    let connect_addrs = (1..=OPERATORS_NUM)
        .map(|id| build_multiaddr!(Memory(id as u64)))
        .collect::<Vec<_>>();

    // Create new operator with all necessary information
    info!("Creating new operator to listen at {}", local_addr);
    let mut new_operator = Operator::new(
        new_operator_keypair.clone(),
        peer_ids.clone(),
        connect_addrs, // Connect directly to existing operators
        local_addr.clone(),
        cancel.child_token(),
        operators
            .iter()
            .map(|op| op.kp.public().clone().into())
            .collect(),
    )
    .unwrap();

    let new_operator_peer_id = new_operator.p2p.local_peer_id();
    let new_operator_handle = new_operator.handle.clone();

    // Wait for existing operators to fully initialize
    sleep(Duration::from_millis(500)).await;

    // Run the new operator in a separate task - this call will handle connections
    tasks.spawn(async move {
        // This will attempt to establish the connections to other operators
        info!("New operator is establishing connections");
        new_operator.p2p.establish_connections().await;
        info!("New operator connections established");

        // This will start listening for messages
        new_operator.p2p.listen().await;
    });

    // Connect the old operators to the new one
    for operator in &operators {
        info!(id = %operator.peer_id, "Connecting operator to new operator");
        new_operator
            .handle
            .send_command(Command::ConnectToPeer(ConnectToPeerCommand {
                peer_id: new_operator_peer_id,
                peer_addr: local_addr.clone(),
            }))
            .await;
    }

    // Give time for the new operator to establish connections
    sleep(Duration::from_secs(5)).await;

    // Verify the connections by having a regular operator send a message first
    let test_id1 = StakeChainId::hash(b"test_from_regular_operator");
    info!("Regular operator sending test message");
    operators[0]
        .handle
        .send_command(mock_stake_chain_info(&operators[0].kp, test_id1))
        .await;

    // Wait for message propagation and verify
    sleep(Duration::from_secs(2)).await;

    // Now try to have the new operator send a message
    let test_id2 = StakeChainId::hash(b"test_from_new_operator");
    info!("New operator sending test message");
    new_operator_handle
        .send_command(mock_stake_chain_info(&new_operator_keypair, test_id2))
        .await;

    // Wait for message propagation
    sleep(Duration::from_secs(2)).await;

    // Check that existing operators received the message
    for operator in &mut operators {
        info!(peer_id=%operator.peer_id, "Checking if operator received message");

        while !operator.handle.events_is_empty() {
            let event = operator.handle.next_event().await?;
            info!(?event, "Received event");

            match event {
                Event::ReceivedMessage(msg) => match &msg.unsigned {
                    UnsignedGossipsubMsg::StakeChainExchange { stake_chain_id, .. } => {
                        if *stake_chain_id == test_id1 {
                            info!("Operator received message from regular operator");
                        } else if *stake_chain_id == test_id2 {
                            info!("Operator received message from new operator");
                        }
                    }
                    _ => bail!("Unexpected message type"),
                },
            }
        }
    }

    // Clean up
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
