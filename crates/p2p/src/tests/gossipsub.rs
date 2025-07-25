//! Gossipsub tests.

use std::time::Duration;

use anyhow::bail;
use futures::SinkExt;
use tokio::time::sleep;

use super::common::Setup;
use crate::{commands::Command, events::GossipEvent, tests::common::init_tracing};

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
