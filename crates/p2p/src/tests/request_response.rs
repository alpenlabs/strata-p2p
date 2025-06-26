//! Request-response tests

use std::time::Duration;

use tokio::time::sleep;

use super::common::Setup;
use crate::{commands::Command, events::Event};

/// Tests the gossip protocol in an all to all connected network with multiple IDs.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn request_response_basic() -> anyhow::Result<()> {
    const OPERATORS_NUM: usize = 2;

    let Setup {
        mut operators,
        cancel,
        tasks,
    } = Setup::all_to_all(OPERATORS_NUM).await?;

    let _ = sleep(Duration::from_nanos(1000000)).await;

    operators[0]
        .handle
        .send_command(Command::RequestMessage {
            peer_id: operators[1].peer_id,
            data: Vec::<u8>::from("Hello, it's a request..."),
        })
        .await;

    match operators[1].handle.next_event().await {
        Ok(event) => match event {
            Event::ReceivedMessage(_data) => {
                panic!(
                    "Something is insanely wrong: it got a gossipsub's message when it should have got a request from Request-response protocol.",
                );
            }
            Event::ReceivedRequest(data) => {
                assert_eq!(data, Vec::<u8>::from("Hello, it's a request..."));
            }
        },
        Err(e) => panic!("Smth is wrong: {e}"),
    }

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}
