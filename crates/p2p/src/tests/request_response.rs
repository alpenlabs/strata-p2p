//! Request-response tests

use std::time::Duration;

use anyhow::bail;
use tokio::time::sleep;
use tracing_test::traced_test;

use super::common::Setup;
use crate::{
    commands::Command, events::Event,
    tests::common::MULTIADDR_MEMORY_ID_OFFSET_REQUEST_RESPONSE_BASIC,
};

/// Tests the gossip protocol in an all to all connected network with multiple IDs.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
#[traced_test]
async fn request_response_basic() -> anyhow::Result<()> {
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM, MULTIADDR_MEMORY_ID_OFFSET_REQUEST_RESPONSE_BASIC).await?;

    let _ = sleep(Duration::from_secs(1)).await;

    user_handles[0]
        .handle
        .send_command(Command::RequestMessage {
            peer_id: user_handles[1].peer_id,
            data: Vec::<u8>::from("Hello, it's a request..."),
        })
        .await;

    match user_handles[1].handle.next_event().await {
        Ok(event) => match event {
            Event::ReceivedMessage(_data) => {
                bail!(
                    "Something is insanely wrong: it got a gossipsub's message when it should have got a request from Request-response protocol.",
                );
            }
            Event::ReceivedRequest(data) => {
                assert_eq!(
                    String::from_utf8(data),
                    String::from_utf8(Vec::<u8>::from("Hello, it's a request..."))
                );
            }
        },
        Err(e) => bail!("Something is wrong: {e}"),
    }

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}
