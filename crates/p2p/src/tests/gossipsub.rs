//! Gossipsub tests.

use std::time::Duration;

use anyhow::bail;
use futures::{SinkExt, StreamExt};
use tokio::time::sleep;
use tokio_stream::wrappers::BroadcastStream;

use super::common::Setup;
use crate::{commands::GossipCommand, events::GossipEvent, tests::common::init_tracing};

/// Tests the gossip protocol in an all to all connected network with multiple IDs.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn gossip_basic() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

    let _ = sleep(Duration::from_secs(1)).await;

    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: ("hello").into(),
        })
        .await
        .expect("Failed to send gossip message");

    match user_handles[1].gossip.next_event().await {
        Ok(event) => match event {
            GossipEvent::ReceivedMessage(data) => {
                assert_eq!(data, Vec::<u8>::from("hello"));
            }
        },
        Err(e) => bail!("Something is wrong: {e}"),
    }

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}

/// Tests the `get_new_receiver` function with `BroadcastStream` to receive gossip events.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn gossip_broadcast_stream() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

    let _ = sleep(Duration::from_secs(1)).await;

    // Create a BroadcastStream from the new receiver
    let receiver = user_handles[1].gossip.get_new_receiver();
    let mut broadcast_stream = BroadcastStream::new(receiver);

    // Send a gossip message from the first user
    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: ("hello from broadcast").into(),
        })
        .await
        .expect("Failed to send gossip message");

    // Receive the message using BroadcastStream
    match broadcast_stream.next().await {
        Some(Ok(GossipEvent::ReceivedMessage(data))) => {
            assert_eq!(data, Vec::<u8>::from("hello from broadcast"));
        }
        Some(Err(e)) => bail!("Error receiving from BroadcastStream: {e}"),
        None => bail!("BroadcastStream ended unexpectedly"),
    }

    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
