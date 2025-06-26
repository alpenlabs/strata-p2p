use std::time::Duration;

use libp2p::PeerId;
use tokio::{sync::oneshot, time::sleep};

use super::common::Setup;
use crate::commands::{Command, QueryP2PStateCommand};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_is_connected() -> anyhow::Result<()> {
    // Set up two connected operators
    let Setup {
        operators,
        cancel,
        tasks,
    } = Setup::all_to_all(2).await?;

    let _ = sleep(Duration::from_nanos(1000000)).await;

    // Verify operator 0 is connected to operator 1
    let is_connected = operators[0].handle.is_connected(operators[1].peer_id).await;
    assert!(is_connected);

    // Also test the get_connected_peers API
    let connected_peers = operators[0].handle.get_connected_peers().await;
    assert!(connected_peers.contains(&operators[1].peer_id));

    // Cleanup
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}

/// Tests the gossip protocol in an all to all connected network with multiple IDs.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn test_manually_get_all_peers() -> anyhow::Result<()> {
    const OPERATORS_NUM: usize = 6;

    let Setup {
        operators,
        cancel,
        tasks,
    } = Setup::all_to_all(OPERATORS_NUM).await?;

    let (tx, rx) = oneshot::channel::<Vec<PeerId>>();

    let _ = sleep(Duration::from_nanos(2000000)).await;

    operators[0]
        .handle
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetConnectedPeers {
                response_sender: tx,
            },
        ))
        .await;

    match rx.await {
        Ok(v) => assert_eq!(v.len(), OPERATORS_NUM - 1),
        Err(e) => panic!("error {e}"),
    };

    assert!(operators[0].handle.events_is_empty());

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}
