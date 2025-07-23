//! Gossipsub tests.

use std::time::Duration;

use anyhow::bail;
use tokio::time::sleep;
use tracing_test::traced_test;

use super::common::{MULTIADDR_MEMORY_ID_OFFSET_GOSSIP_BASIC, Setup};
use crate::{commands::Command, events::GossipEvent};

/// Tests the gossip protocol in an all to all connected network with multiple IDs.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
#[traced_test]
async fn gossip_basic() -> anyhow::Result<()> {
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM, MULTIADDR_MEMORY_ID_OFFSET_GOSSIP_BASIC).await?;

    let _ = sleep(Duration::from_secs(1)).await;

    user_handles[0]
        .command
        .send_command(Command::PublishMessage {
            data: ("hello").into(),
        })
        .await;

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
