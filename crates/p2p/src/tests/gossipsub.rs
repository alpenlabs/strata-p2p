//! Gossipsub tests.

use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use super::common::Setup;

// /// Tests the gossip protocol in an all to all connected network with a single ID.
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
        operators: mut _operators,
        cancel,
        tasks,
    } = Setup::all_to_all(OPERATORS_NUM).await?;

    // TODO(Arniiiii): do some logic for testing..

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}
