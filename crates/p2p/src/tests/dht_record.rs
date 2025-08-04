//! Test that checks if we really put something to DHT.

use std::time::Duration;

use anyhow::bail;
use tokio::{sync::oneshot, time::sleep};

use super::common::Setup;
use crate::{commands::Command, tests::common::init_tracing};

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_dht_record() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 10;

    let Setup {
        user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

    sleep(Duration::from_secs(5)).await;

    let (tx, rx) = oneshot::channel();

    user_handles[0]
        .command
        .send_command(Command::GetDHTRecord {
            app_public_key: user_handles[USERS_NUM - 1].app_keypair.public(),
            response_sender: tx,
        })
        .await;

    match rx.await {
        Ok(smth) => {
            if smth.is_none() {
                bail!(
                    "Last user haven't posted a valid record, from first user's point of view. That's unfortunate."
                );
            };
        }
        Err(e) => bail!("Something is wrong: {e}"),
    }

    cancel.cancel();

    tasks.wait().await;

    Ok(())
}
