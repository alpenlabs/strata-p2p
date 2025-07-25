use std::time::Duration;

use anyhow::bail;
use libp2p::PeerId;
use tokio::{sync::oneshot, time::sleep};

use super::common::Setup;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    tests::common::init_tracing,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_is_connected() -> anyhow::Result<()> {
    init_tracing();
    // Set up two connected user_handles
    let Setup {
        user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(2).await?;

    let _ = sleep(Duration::from_secs(1)).await;

    // Verify user 0 is connected to user 1
    let user1_app_pk = user_handles[1].app_keypair.public();
    let is_connected = user_handles[0]
        .command
        .is_connected(&user1_app_pk, None)
        .await;
    assert!(is_connected);

    let (tx, rx) = oneshot::channel::<bool>();

    // Verify user 0 is connected to user 1 manually
    user_handles[0]
        .command
        .send_command(Command::from(QueryP2PStateCommand::IsConnected {
            app_public_key: user1_app_pk,
            response_sender: tx,
        }))
        .await;
    let is_connected = rx.await.unwrap();
    assert!(is_connected);

    // Also test the get_connected_peers API
    let connected_peers = user_handles[0].command.get_connected_peers(None).await;
    assert!(connected_peers.contains(&user_handles[1].peer_id));

    let (tx, rx) = oneshot::channel::<Vec<PeerId>>();
    // Also test the get_connected_peers API manually
    user_handles[0]
        .command
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
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn test_manually_get_all_peers() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 6;

    let Setup {
        user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

    let (tx, rx) = oneshot::channel::<Vec<PeerId>>();

    let _ = sleep(Duration::from_secs(2)).await;

    user_handles[0]
        .command
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetConnectedPeers {
                response_sender: tx,
            },
        ))
        .await;

    match rx.await {
        Ok(v) => assert_eq!(v.len(), USERS_NUM - 1),
        Err(e) => bail!("error {e}"),
    };

    #[cfg(feature = "gossipsub")]
    assert!(user_handles[0].gossip.events_is_empty());

    #[cfg(feature = "request-response")]
    assert!(user_handles[0].reqresp.events_is_empty());

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}
