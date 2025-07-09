use std::time::Duration;

use anyhow::bail;
use libp2p::PeerId;
use tokio::{
    sync::oneshot,
    time::{sleep, timeout},
};
use tracing::info;
use tracing_test::traced_test;

use super::common::Setup;
use crate::commands::{Command, QueryP2PStateCommand};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[traced_test]
async fn test_is_connected() -> anyhow::Result<()> {
    // Set up two connected user_handles
    let Setup {
        user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(2, 150).await?;

    let _ = sleep(Duration::from_secs(1)).await;

    // Verify user 0 is connected to user 1
    let is_connected = user_handles[0]
        .handle
        .is_connected(user_handles[1].peer_id)
        .await;
    assert!(is_connected);

    let (tx, rx) = oneshot::channel::<bool>();

    // Verify user 0 is connected to user 1 manually
    user_handles[0]
        .handle
        .send_command(Command::from(QueryP2PStateCommand::IsConnected {
            peer_id: user_handles[1].peer_id,
            response_sender: tx,
        }))
        .await;
    let is_connected = rx.await.unwrap();
    assert!(is_connected);

    // Also test the get_connected_peers API
    let connected_peers = user_handles[0].handle.get_connected_peers().await;
    assert!(connected_peers.contains(&user_handles[1].peer_id));

    let (tx, rx) = oneshot::channel::<Vec<PeerId>>();
    // Also test the get_connected_peers API manually
    user_handles[0]
        .handle
        .send_command(Command::from(QueryP2PStateCommand::GetConnectedPeers {
            response_sender: tx,
        }))
        .await;
    let connected_peers = rx.await.unwrap();
    assert!(connected_peers.contains(&user_handles[1].peer_id));

    // Cleanup
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}

/// Tests the gossip protocol in an all to all connected network with multiple IDs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[traced_test]
async fn test_manually_get_all_peers() -> anyhow::Result<()> {
    const USERS_NUM: usize = 10;

    info!("Setupping users");
    let Setup {
        user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM, 500).await?;

    info!("Waiting for users to setup...");
    sleep(Duration::from_secs(2)).await;

    info!(
        "Creating oneshot channel for command Command::QueryP2PState(QueryP2PStateCommand::GetConnectedPeers"
    );
    let (tx, rx) = oneshot::channel::<Vec<PeerId>>();

    info!("Sending command Command::QueryP2PState(QueryP2PStateCommand::GetConnectedPeers");

    user_handles[0]
        .handle
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetConnectedPeers {
                response_sender: tx,
            },
        ))
        .await;

    info!(
        "Waiting for result from command Command::QueryP2PState(QueryP2PStateCommand::GetConnectedPeers"
    );

    match timeout(Duration::from_secs(1), rx).await {
        Ok(v) => assert_eq!(v.unwrap().len(), USERS_NUM - 1),
        Err(e) => bail!("error {e}"),
    };

    assert!(user_handles[0].handle.events_is_empty());

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}
