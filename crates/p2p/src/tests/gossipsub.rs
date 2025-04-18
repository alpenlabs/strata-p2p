//! Gossipsub tests.

use std::{collections::HashSet, time::Duration};

use strata_p2p_types::{Scope, SessionId, StakeChainId};
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use super::common::{
    exchange_deposit_nonces, exchange_deposit_setup, exchange_deposit_sigs,
    exchange_stake_chain_info, Setup,
};
use crate::{events::Event, tests::common::mock_stake_chain_info};

/// Tests the gossip protocol in an all to all connected network with a single ID.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn all_to_all_one_id() -> anyhow::Result<()> {
    const OPERATORS_NUM: usize = 2;

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let Setup {
        mut operators,
        cancel,
        tasks,
    } = Setup::all_to_all(OPERATORS_NUM).await?;

    let stake_chain_id = StakeChainId::hash(b"stake_chain_id");
    let scope = Scope::hash(b"scope");
    let session_id = SessionId::hash(b"session_id");

    exchange_stake_chain_info(&mut operators, OPERATORS_NUM, stake_chain_id).await?;
    exchange_deposit_setup(&mut operators, OPERATORS_NUM, scope).await?;
    exchange_deposit_nonces(&mut operators, OPERATORS_NUM, session_id).await?;
    exchange_deposit_sigs(&mut operators, OPERATORS_NUM, session_id).await?;

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}

/// Tests the gossip protocol in an all to all connected network with multiple IDs.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn all_to_all_multiple_ids() -> anyhow::Result<()> {
    const OPERATORS_NUM: usize = 2;

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let Setup {
        mut operators,
        cancel,
        tasks,
    } = Setup::all_to_all(OPERATORS_NUM).await?;

    let stake_chain_ids = (0..OPERATORS_NUM)
        .map(|i| StakeChainId::hash(format!("stake_chain_id_{}", i).as_bytes()))
        .collect::<Vec<_>>();
    let scopes = (0..OPERATORS_NUM)
        .map(|i| Scope::hash(format!("scope_{}", i).as_bytes()))
        .collect::<Vec<_>>();

    let session_ids = (0..OPERATORS_NUM)
        .map(|i| SessionId::hash(format!("session_{}", i).as_bytes()))
        .collect::<Vec<_>>();

    for stake_chain_id in &stake_chain_ids {
        exchange_stake_chain_info(&mut operators, OPERATORS_NUM, *stake_chain_id).await?;
    }

    for scope in &scopes {
        exchange_deposit_setup(&mut operators, OPERATORS_NUM, *scope).await?;
    }
    for session_id in &session_ids {
        exchange_deposit_nonces(&mut operators, OPERATORS_NUM, *session_id).await?;
    }
    for session_id in &session_ids {
        exchange_deposit_sigs(&mut operators, OPERATORS_NUM, *session_id).await?;
    }

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}

/// Tests the gossip protocol in a connected network with multiple operators and a single ID and
/// checks if all the nodes (including the one first publishing it) receive it.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn all_to_all_single_id_self_recv() -> anyhow::Result<()> {
    const OPERATORS_NUM: usize = 3;
    const WAIT_TIME: Duration = Duration::from_secs(10);

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let Setup {
        mut operators,
        cancel,
        tasks,
    } = Setup::all_to_all(OPERATORS_NUM).await?;

    let stake_chain_id = StakeChainId::hash(b"stake_chain_id");

    // Track received messages per operator
    let mut received_msgs: Vec<HashSet<String>> = vec![HashSet::new(); OPERATORS_NUM];

    let sender = &operators[0];

    // Send message from first operator
    info!("sending out stake chain info message");
    sender
        .handle
        .send_command(mock_stake_chain_info(&operators[0].kp, stake_chain_id))
        .await;

    // Wait some time to collect messages
    tokio::time::sleep(WAIT_TIME).await;

    // Check received messages for each operator except the sender
    for (i, operator) in operators.iter_mut().enumerate() {
        while !operator.handle.events_is_empty() {
            let event = operator.handle.next_event().await?;
            if let Event::ReceivedMessage(msg) = event {
                // Create a unique message identifier from the content
                let msg_content = hex::encode(msg.content());

                // if received_msgs[i].contains(&msg_content) {
                //     panic!("Operator {} received duplicate message: {}", i, msg_content);
                // }
                received_msgs[i].insert(msg_content);
            }
        }
    }

    // Verify each operator received exactly one message
    for (i, msgs) in received_msgs.iter().enumerate() {
        assert_eq!(
            msgs.len(),
            1,
            "Operator {} received {} messages, expected 1",
            i,
            msgs.len()
        );
    }

    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
