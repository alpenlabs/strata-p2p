//! Test that checks if DHT records works as expected

use std::time::Duration;

use anyhow::bail;
use tokio::time::sleep;

use super::common::Setup;
use crate::{
    commands::Command,
    events::ReqRespEvent,
    tests::common::{MULTIADDR_MEMORY_ID_OFFSET_TEST_DHT_RECORD, init_tracing},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_dht_record() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 10;

    const INITIAL_LIMIT_OF_USERS_TO_CONNECT_TO: usize = 4;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_limited(
        USERS_NUM,
        INITIAL_LIMIT_OF_USERS_TO_CONNECT_TO,
        MULTIADDR_MEMORY_ID_OFFSET_TEST_DHT_RECORD,
    )
    .await?;

    sleep(Duration::from_secs(5)).await;

    user_handles[0]
        .command
        .send_command(Command::RequestMessage {
            app_public_key: user_handles[USERS_NUM - 1].app_keypair.public(),
            data: "hello".into(),
        })
        .await;

    match user_handles[USERS_NUM - 1].reqresp.next_event().await {
        Some(event) => {
            if let ReqRespEvent::ReceivedRequest ( data, sender ) = event {
                assert_eq!(data, Vec::<u8>::from("hello"));
                let _ = sender.send("received hello".into());
            }
        }
        None => bail!("smth is wrong"),
    }

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}
