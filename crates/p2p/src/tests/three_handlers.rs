//! Three-handlers integration test for new handler-based architecture.

use chrono as _;
use tracing::info;

use super::common::Setup;
use crate::{commands::Command, events::ReqRespEvent};

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_reqresp_basic() -> anyhow::Result<()> {
    let Setup {
        cancel,
        mut user_handles,
        tasks,
    } = Setup::all_to_all(2).await?;

    let req_msg = b"request from node1".to_vec();
    user_handles[0]
        .command
        .send_command(Command::RequestMessage {
            peer_id: user_handles[1].peer_id,
            data: req_msg.clone(),
        })
        .await;
    info!("Node 1 sent request to Node 2");

    match user_handles[1]
        .reqresp
        .as_mut()
        .unwrap()
        .next_event()
        .await
        .unwrap()
    {
        ReqRespEvent::ReceivedRequest(data, responder) => {
            info!("Node 2 received request: {:?}", data);
            assert_eq!(data, req_msg, "Node 2 did not receive the correct request");
            let _ = responder.send(b"response from node3".to_vec());
            info!("[TEST] Node 2 sent response");
        }
        _ => anyhow::bail!("Node 2 did not receive a CustomEvent request"),
    }

    match user_handles[0]
        .reqresp
        .as_mut()
        .unwrap()
        .next_event()
        .await
        .unwrap()
    {
        ReqRespEvent::ReceivedMessage(resp) => {
            println!("[TEST] Node 1 received response: {:?}", resp);
            assert_eq!(
                resp,
                b"response from node3".to_vec(),
                "Node 1 did not receive the correct response"
            );
        }
        _ => anyhow::bail!("Node 1 did not receive a response message"),
    }

    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
