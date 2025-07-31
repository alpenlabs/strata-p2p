//! Test that checks if request in request response protocol can use DHT to get multiaddress and
//! transport id (aka peer id) and send the request via knowledge of this.

use std::time::Duration;

use anyhow::bail;
use futures::SinkExt;
use tokio::time::sleep;

use super::common::Setup;
use crate::{
    commands::RequestResponseCommand,
    events::ReqRespEvent,
    tests::common::{MULTIADDR_MEMORY_ID_OFFSET_TEST_REQUEST_RESPONSE_DHT, init_tracing},
};

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[ignore = "Because this test is impossible to run on my 12 thread laptop without getting `OutboundUpgradeError(Timeout)` from any behaviour, even when worker threads are 12 and timeouts set to 10secs. To run this test you need a threadripper beast and smth like 100 GB of RAM."]
async fn test_request_response_dht() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 1500; // the amount here should be more than
    // k_which_is_by_default_4_AFAI_guess *
    // amount_of_bits_in_hash_of_DHT_which_is_for_libp2p_256.

    const INITIAL_LIMIT_OF_USERS_TO_CONNECT_TO: usize = 100; // because if to use plain all_to_all,
    // this test does too much useless
    // connections.

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_limited(
        USERS_NUM,
        INITIAL_LIMIT_OF_USERS_TO_CONNECT_TO,
        MULTIADDR_MEMORY_ID_OFFSET_TEST_REQUEST_RESPONSE_DHT,
    )
    .await?;

    sleep(Duration::from_secs(5)).await;

    // And even this does not guarantee that this test will use DHT to connect to the last guy,
    // because distance between first and last guys in DHT's space can be small because the
    // distance depends on xor(hash(app_public_key),hash(app_public_key)), not on what order we
    // created them. This is unfortunate.
    let (first, others) = user_handles.split_at_mut(1);
    first[0]
        .reqresp
        .send(RequestResponseCommand {
            target_app_public_key: others[USERS_NUM - 2].app_keypair.public(),
            data: "hello".into(),
        })
        .await
        .expect("Failed to send request message");

    match user_handles[USERS_NUM - 1].reqresp.next_event().await {
        Some(event) => {
            if let ReqRespEvent::ReceivedRequest(data, sender) = event {
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
