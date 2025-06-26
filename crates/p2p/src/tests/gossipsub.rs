//! Gossipsub tests.

use std::time::Duration;

use tokio::time::sleep;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use super::common::Setup;
use crate::{commands::Command, events::Event};

/// Tests the gossip protocol in an all to all connected network with multiple IDs.
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn gossip_basic() -> anyhow::Result<()> {
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

    let _ = sleep(Duration::from_nanos(1000000)).await;

    operators[0]
        .handle
        .send_command(Command::PublishMessage {
            data: ("hello").into(),
        })
        .await;

    match operators[1].handle.next_event().await {
        Ok(event) => match event {
            Event::ReceivedMessage(data) => {
                assert_eq!(data, Vec::<u8>::from("hello"));
            }
            Event::ReceivedRequest(_) => {
                panic!(
                    "Something is insanely wrong: it got request when it should have got just a message."
                )
            }
        },
        Err(e) => panic!("Smth is wrong: {e}"),
    }

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}
