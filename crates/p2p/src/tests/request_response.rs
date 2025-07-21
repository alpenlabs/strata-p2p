//! Request-response tests

use tracing::info;

use super::common::Setup;
use crate::{commands::Command, events::ReqRespEvent, tests::common::init_tracing};

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_reqresp_basic() -> anyhow::Result<()> {
    init_tracing();

    let Setup {
        cancel,
        mut user_handles,
        tasks,
    } = Setup::all_to_all(2).await?;

    let req_msg = b"request from node1".to_vec();
    let resp_msg = b"response from node2".to_vec();
    user_handles[0]
        .command
        .send_command(Command::RequestMessage {
            peer_id: user_handles[1].peer_id,
            data: req_msg.clone(),
        })
        .await;
    info!("Node 1 sent request to Node 2");

    match user_handles[1].reqresp.next_event().await.unwrap() {
        ReqRespEvent::ReceivedRequest(data, channel) => {
            info!(?data, "Node 2 received request");
            assert_eq!(data, req_msg, "Node 2 did not receive the correct request");
            let _ = channel.send(resp_msg.clone());
            info!("Node 2 sent response");
        }
        _ => unreachable!("Node 2 did not receive a request"),
    }

    match user_handles[0].reqresp.next_event().await.unwrap() {
        ReqRespEvent::ReceivedResponse(resp) => {
            info!(?resp, "Node 1 received response");
            assert_eq!(
                resp, resp_msg,
                "Node 1 did not receive the correct response",
            );
        }
        _ => unreachable!("Node 1 did not receive a response"),
    }

    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
