//! Gossipsub tests.

<<<<<<< HEAD
use strata_p2p_types::{Scope, SessionId};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use super::common::Setup;

/// Tests the gossip protocol in an all to all connected network with a single ID.
// #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
// async fn all_to_all_one_id() -> anyhow::Result<()> {
//     const OPERATORS_NUM: usize = 2;
//
//     tracing_subscriber::registry()
//         .with(fmt::layer())
//         .with(EnvFilter::from_default_env())
//         .init();
//
//     let Setup {
//         mut operators,
//         cancel,
//         tasks,
//     } = Setup::all_to_all(OPERATORS_NUM).await?;
//
//     let stake_chain_id = StakeChainId::hash(b"stake_chain_id");
//     let scope = Scope::hash(b"scope");
//     let session_id = SessionId::hash(b"session_id");
//
//     exchange_stake_chain_info(&mut operators, OPERATORS_NUM, stake_chain_id).await?;
//     exchange_deposit_setup(&mut operators, OPERATORS_NUM, scope).await?;
//     exchange_deposit_nonces(&mut operators, OPERATORS_NUM, session_id).await?;
//     exchange_deposit_sigs(&mut operators, OPERATORS_NUM, session_id).await?;
//
//     cancel.cancel();
//
//     tasks.wait().await;
//
//     Ok(())
// }

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
    //
    // let stake_chain_ids = (0..OPERATORS_NUM)
    //     .map(|i| StakeChainId::hash(format!("stake_chain_id_{}", i).as_bytes()))
    //     .collect::<Vec<_>>();
    // let scopes = (0..OPERATORS_NUM)
    //     .map(|i| Scope::hash(format!("scope_{}", i).as_bytes()))
    //     .collect::<Vec<_>>();
    //
    // let session_ids = (0..OPERATORS_NUM)
    //     .map(|i| SessionId::hash(format!("session_{}", i).as_bytes()))
    //     .collect::<Vec<_>>();
    //
    // for stake_chain_id in &stake_chain_ids {
    //     exchange_stake_chain_info(&mut operators, OPERATORS_NUM, *stake_chain_id).await?;
    // }
    //
    // for scope in &scopes {
    //     exchange_deposit_setup(&mut operators, OPERATORS_NUM, *scope).await?;
    // }
    // for session_id in &session_ids {
    //     exchange_deposit_nonces(&mut operators, OPERATORS_NUM, *session_id).await?;
    // }
    // for session_id in &session_ids {
    //     exchange_deposit_sigs(&mut operators, OPERATORS_NUM, *session_id).await?;
    // }
=======
use std::time::Duration;

use anyhow::bail;
use tokio::time::sleep;

use super::common::Setup;
use crate::{commands::Command, events::Event};
/// Tests the gossip protocol in an all to all connected network with multiple IDs.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn gossip_basic() -> anyhow::Result<()> {
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

    let _ = sleep(Duration::from_secs(1)).await;

    user_handles[0]
        .handle
        .send_command(Command::PublishMessage {
            data: ("hello").into(),
        })
        .await;

    match user_handles[1].handle.next_event().await {
        Ok(event) => match event {
            Event::ReceivedMessage(data) => {
                assert_eq!(data, Vec::<u8>::from("hello"));
            }
            Event::ReceivedRequest(_) => {
                bail!(
                    "Something is insanely wrong: it got request when it should have got just a message."
                )
            }
        },
        Err(e) => bail!("Something is wrong: {e}"),
    }
>>>>>>> dev-v2

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}
