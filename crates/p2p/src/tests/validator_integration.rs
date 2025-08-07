//! Integration tests for the validator and score manager.

use std::time::Duration;

use futures::SinkExt;
use tokio::{
    sync::oneshot,
    time::{sleep, timeout},
};
use tracing::info;

use super::common::{Setup, init_tracing};
#[cfg(feature = "request-response")]
use crate::{commands::RequestResponseCommand, events::ReqRespEvent};
use crate::{
    commands::{Command, GossipCommand, QueryP2PStateCommand},
    events::GossipEvent,
    validator::{Message, PenaltyType, Validator},
};

#[derive(Debug, Default, Clone)]
struct TestValidator;

impl Validator for TestValidator {
    #[allow(unused_variables)]
    fn validate_msg(&self, msg: &Message, old_app_score: f64) -> f64 {
        0.0
    }

    #[allow(unused_variables)]
    fn get_penalty(
        &self,
        msg: &Message,
        gossip_internal_score: f64,
        gossip_app_score: f64,
        reqresp_app_score: f64,
    ) -> Option<PenaltyType> {
        match msg {
            Message::Gossipsub(data) | Message::Request(data) | Message::Response(data) => {
                let content = String::from_utf8_lossy(data);
                match content.as_ref() {
                    "ignore me" => Some(PenaltyType::Ignore),
                    "mute gossip" => Some(PenaltyType::MuteGossip(Duration::from_secs(4))),
                    "mute reqresp" => Some(PenaltyType::MuteReqresp(Duration::from_secs(4))),
                    "mute both" => Some(PenaltyType::MuteBoth(Duration::from_secs(4))),
                    "ban me" => Some(PenaltyType::Ban(Some(Duration::from_secs(5)))),
                    _ => None,
                }
            }
        }
    }
}

#[cfg(feature = "gossipsub")]
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_gossipsub_mute_penalty() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_with_custom_validator(USERS_NUM, TestValidator).await?;

    sleep(Duration::from_millis(500)).await;

    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: "normal message".into(),
        })
        .await?;

    let GossipEvent::ReceivedMessage(data) = user_handles[1].gossip.next_event().await?;
    assert_eq!(data, b"normal message");
    info!("Normal message received before mute");
    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: "mute gossip".into(),
        })
        .await?;

    sleep(Duration::from_millis(200)).await;
    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: "this should be muted".into(),
        })
        .await?;

    let result = timeout(Duration::from_secs(1), user_handles[1].gossip.next_event()).await;

    match result {
        Err(_) => info!("Timeout occurred - peer is correctly muted for gossip"),
        Ok(Ok(event)) => {
            let GossipEvent::ReceivedMessage(data) = event;
            if data == b"this should be muted" {
                panic!("Received muted message - muting failed!");
            }
            info!(?data, "Received different message");
        }
        Ok(Err(e)) => panic!("Error receiving message: {e}"),
    }

    sleep(Duration::from_secs(4)).await;

    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: "after mute expired".into(),
        })
        .await?;

    let GossipEvent::ReceivedMessage(data) = user_handles[1].gossip.next_event().await?;
    assert_eq!(data, b"after mute expired");
    info!("Message sent successfully after mute expired");

    cancel.cancel();
    tasks.wait().await;
    Ok(())
}

#[cfg(feature = "request-response")]
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_reqresp_mute_penalty() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_with_custom_validator(USERS_NUM, TestValidator).await?;

    sleep(Duration::from_millis(500)).await;

    // Split user_handles to avoid borrow checker issues
    let (user0, user1) = user_handles.split_at_mut(1);
    #[cfg(feature = "byos")]
    let target_public_key = user1[0].app_keypair.public().clone();
    #[cfg(not(feature = "byos"))]
    let target_transport_id = user1[0].peer_id;

    user0[0]
        .reqresp
        .send(RequestResponseCommand {
            #[cfg(feature = "byos")]
            target_app_public_key: target_public_key.clone(),
            target_transport_id,
            data: "normal request".into(),
        })
        .await?;

    if let Some(ReqRespEvent::ReceivedRequest(data, _)) = user1[0].reqresp.next_event().await {
        assert_eq!(data, b"normal request");
        info!("Normal request received");
    }
    user0[0]
        .reqresp
        .send(RequestResponseCommand {
            #[cfg(feature = "byos")]
            target_app_public_key: target_public_key.clone(),
            target_transport_id,
            data: "mute reqresp".into(),
        })
        .await?;

    sleep(Duration::from_millis(200)).await;
    user0[0]
        .reqresp
        .send(RequestResponseCommand {
            #[cfg(feature = "byos")]
            target_app_public_key: target_public_key.clone(),
            #[cfg(not(feature = "byos"))]
            target_transport_id,
            data: "this should be muted".into(),
        })
        .await?;

    let result = timeout(Duration::from_millis(1000), user1[0].reqresp.next_event()).await;

    match result {
        Err(_) => info!("Timeout occurred - peer is correctly muted for req/resp"),
        Ok(Some(event)) => {
            if let ReqRespEvent::ReceivedRequest(data, _) = event
                && data == b"this should be muted"
            {
                panic!("Received muted request - muting failed!");
            }
        }
        Ok(None) => info!("No event received - peer is correctly muted"),
    }

    sleep(Duration::from_secs(4)).await;

    user0[0]
        .reqresp
        .send(RequestResponseCommand {
            #[cfg(feature = "byos")]
            target_app_public_key: target_public_key.clone(),
            #[cfg(not(feature = "byos"))]
            target_transport_id,
            data: "after mute expired".into(),
        })
        .await?;

    if let Some(ReqRespEvent::ReceivedRequest(data, _)) = user1[0].reqresp.next_event().await {
        assert_eq!(data, b"after mute expired");
        info!("Request sent successfully after mute expired");
    } else {
        panic!("Expected ReceivedRequest");
    }

    cancel.cancel();
    tasks.wait().await;
    Ok(())
}

#[cfg(feature = "gossipsub")]
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_gossipsub_ban_penalty() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_with_custom_validator(USERS_NUM, TestValidator).await?;

    let _ = sleep(Duration::from_millis(500)).await;

    let (tx, rx) = oneshot::channel();
    let is_connected_query = QueryP2PStateCommand::IsConnected {
        #[cfg(feature = "byos")]
        app_public_key: user_handles[1].app_keypair.public(),
        #[cfg(not(feature = "byos"))]
        transport_id: user_handles[1].peer_id,
        response_sender: tx,
    };

    user_handles[0]
        .command
        .send_command(is_connected_query)
        .await;
    let is_connected = rx.await.expect("Failed to check connection status");
    info!(?is_connected, "connection status");
    assert!(is_connected);

    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: "ban me".into(),
        })
        .await
        .expect("Failed to send ban message");

    let _ = sleep(Duration::from_millis(100)).await;

    let (tx, rx) = oneshot::channel();
    let is_connected_query = QueryP2PStateCommand::IsConnected {
        #[cfg(feature = "byos")]
        app_public_key: user_handles[1].app_keypair.public(),
        #[cfg(not(feature = "byos"))]
        transport_id: user_handles[1].peer_id,
        response_sender: tx,
    };

    user_handles[0]
        .command
        .send_command(is_connected_query)
        .await;
    let is_connected = rx.await.expect("Failed to check connection status");
    info!(?is_connected, "connection status");
    assert!(!is_connected);

    // Wait for the ban period to end
    tokio::time::sleep(Duration::from_secs(5)).await;

    let (tx, rx) = oneshot::channel();
    user_handles[1]
        .command
        .send_command(QueryP2PStateCommand::GetMyListeningAddresses {
            response_sender: tx,
        })
        .await;
    let listening_addresses = rx.await.expect("Failed to get listening addresses");
    info!(?listening_addresses, "listening addresses");
    assert!(!listening_addresses.is_empty());

    user_handles[0]
        .command
        .send_command(Command::ConnectToPeer {
            #[cfg(feature = "byos")]
            app_public_key: user_handles[1].app_keypair.public(),
            #[cfg(not(feature = "byos"))]
            transport_id: user_handles[1].peer_id,
            addresses: listening_addresses,
        })
        .await;

    let _ = sleep(Duration::from_millis(100)).await;

    let (tx, rx) = oneshot::channel();
    user_handles[0]
        .command
        .send_command(Command::QueryP2PState(QueryP2PStateCommand::IsConnected {
            #[cfg(feature = "byos")]
            app_public_key: user_handles[1].app_keypair.public(),
            #[cfg(not(feature = "byos"))]
            transport_id: user_handles[1].peer_id,
            response_sender: tx,
        }))
        .await;
    let is_connected = rx.await.expect("Failed to check connection status");
    info!(?is_connected, "connection status");
    assert!(is_connected);

    cancel.cancel();
    tasks.wait().await;

    Ok(())
}

#[cfg(feature = "request-response")]
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_reqresp_ban_penalty() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_with_custom_validator(USERS_NUM, TestValidator).await?;

    let _ = sleep(Duration::from_millis(500)).await;

    let (tx, rx) = oneshot::channel();
    let is_connected_query = QueryP2PStateCommand::IsConnected {
        #[cfg(feature = "byos")]
        app_public_key: user_handles[1].app_keypair.public(),
        #[cfg(not(feature = "byos"))]
        transport_id: user_handles[1].peer_id,
        response_sender: tx,
    };

    user_handles[0]
        .command
        .send_command(is_connected_query)
        .await;
    let is_connected = rx.await.expect("Failed to check connection status");
    info!(?is_connected, "connection status");
    assert!(is_connected);

    // Split user_handles to avoid borrow checker issues
    let (user0, user1) = user_handles.split_at_mut(1);
    #[cfg(feature = "byos")]
    let target_public_key = user1[0].app_keypair.public().clone();
    #[cfg(not(feature = "byos"))]
    let target_transport_id = user1[0].peer_id;

    user0[0]
        .reqresp
        .send(RequestResponseCommand {
            #[cfg(feature = "byos")]
            target_app_public_key: target_public_key.clone(),
            #[cfg(not(feature = "byos"))]
            target_transport_id,
            data: "ban me".into(),
        })
        .await
        .expect("Failed to send ban message");

    let _ = sleep(Duration::from_millis(500)).await;

    let (tx, rx) = oneshot::channel();
    let is_connected_query = QueryP2PStateCommand::IsConnected {
        #[cfg(feature = "byos")]
        app_public_key: user1[0].app_keypair.public(),
        #[cfg(not(feature = "byos"))]
        transport_id: user1[0].peer_id,
        response_sender: tx,
    };

    user0[0].command.send_command(is_connected_query).await;

    let is_connected = rx.await.expect("Failed to check connection status");
    info!(?is_connected, "connection status");
    assert!(!is_connected);

    // Wait for the ban period to end
    tokio::time::sleep(Duration::from_secs(5)).await;

    let (tx, rx) = oneshot::channel();
    user1[0]
        .command
        .send_command(QueryP2PStateCommand::GetMyListeningAddresses {
            response_sender: tx,
        })
        .await;
    let listening_addresses = rx.await.expect("Failed to get listening addresses");
    info!(?listening_addresses, "listening addresses");
    assert!(!listening_addresses.is_empty());

    user0[0]
        .command
        .send_command(Command::ConnectToPeer {
            #[cfg(feature = "byos")]
            app_public_key: user1[0].app_keypair.public(),
            #[cfg(not(feature = "byos"))]
            transport_id: user1[0].peer_id,
            addresses: listening_addresses,
        })
        .await;

    let _ = sleep(Duration::from_millis(100)).await;

    let (tx, rx) = oneshot::channel();
    user0[0]
        .command
        .send_command(Command::QueryP2PState(QueryP2PStateCommand::IsConnected {
            #[cfg(feature = "byos")]
            app_public_key: user1[0].app_keypair.public(),
            #[cfg(not(feature = "byos"))]
            transport_id: user1[0].peer_id,
            response_sender: tx,
        }))
        .await;
    let is_connected = rx.await.expect("Failed to check connection status");
    info!(?is_connected, "connection status");
    assert!(is_connected);

    cancel.cancel();
    tasks.wait().await;

    Ok(())
}

#[cfg(feature = "gossipsub")]
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_gossipsub_ignore_penalty() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_with_custom_validator(USERS_NUM, TestValidator).await?;

    sleep(Duration::from_millis(500)).await;

    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: "ignore me".into(),
        })
        .await?;

    sleep(Duration::from_millis(200)).await;

    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: "normal message after ignore".into(),
        })
        .await?;

    let GossipEvent::ReceivedMessage(data) = user_handles[1].gossip.next_event().await?;
    assert_eq!(data, b"normal message after ignore");
    info!("Normal message received after ignore penalty - ignore working correctly");

    cancel.cancel();
    tasks.wait().await;
    Ok(())
}

#[cfg(feature = "gossipsub")]
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_gossipsub_mute_both_penalty() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_with_custom_validator(USERS_NUM, TestValidator).await?;

    sleep(Duration::from_millis(500)).await;

    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: "normal message".into(),
        })
        .await?;

    let GossipEvent::ReceivedMessage(data) = user_handles[1].gossip.next_event().await?;
    assert_eq!(data, b"normal message");
    info!("Normal message received before mute");

    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: "mute both".into(),
        })
        .await?;

    sleep(Duration::from_millis(200)).await;

    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: "this should be muted gossip".into(),
        })
        .await?;

    let result = timeout(Duration::from_secs(1), user_handles[1].gossip.next_event()).await;
    match result {
        Err(_) => info!("Timeout occurred - peer is correctly muted for gossip"),
        Ok(Ok(event)) => {
            let GossipEvent::ReceivedMessage(data) = event;
            if data == b"this should be muted gossip" {
                panic!("Received muted gossip message - muting failed!");
            }
            info!(?data, "Received different message");
        }
        Ok(Err(e)) => panic!("Error receiving message: {e}"),
    }

    sleep(Duration::from_secs(4)).await;

    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: "after mute expired".into(),
        })
        .await?;

    let GossipEvent::ReceivedMessage(data) = user_handles[1].gossip.next_event().await?;
    assert_eq!(data, b"after mute expired");
    info!("Message sent successfully after mute expired");

    cancel.cancel();
    tasks.wait().await;
    Ok(())
}

#[cfg(all(feature = "request-response", feature = "gossipsub"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_reqresp_mute_both_penalty() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 2;

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_with_custom_validator(USERS_NUM, TestValidator).await?;

    sleep(Duration::from_millis(500)).await;

    let (user0, user1) = user_handles.split_at_mut(1);
    #[cfg(feature = "byos")]
    let target_public_key = user1[0].app_keypair.public().clone();
    #[cfg(not(feature = "byos"))]
    let target_transport_id = user1[0].peer_id;

    user0[0]
        .reqresp
        .send(RequestResponseCommand {
            #[cfg(feature = "byos")]
            target_app_public_key: target_public_key.clone(),
            #[cfg(not(feature = "byos"))]
            target_transport_id,
            data: "normal request".into(),
        })
        .await?;

    if let Some(ReqRespEvent::ReceivedRequest(data, _)) = user1[0].reqresp.next_event().await {
        assert_eq!(data, b"normal request");
        info!("Normal request received");
    }

    user0[0]
        .reqresp
        .send(RequestResponseCommand {
            #[cfg(feature = "byos")]
            target_app_public_key: target_public_key.clone(),
            #[cfg(not(feature = "byos"))]
            target_transport_id,
            data: "mute both".into(),
        })
        .await?;

    sleep(Duration::from_millis(200)).await;

    user0[0]
        .reqresp
        .send(RequestResponseCommand {
            #[cfg(feature = "byos")]
            target_app_public_key: target_public_key.clone(),
            #[cfg(not(feature = "byos"))]
            target_transport_id,
            data: "this should be muted reqresp".into(),
        })
        .await?;

    let result = timeout(Duration::from_millis(1000), user1[0].reqresp.next_event()).await;
    match result {
        Err(_) => info!("Timeout occurred - peer is correctly muted for req/resp"),
        Ok(Some(event)) => {
            if let ReqRespEvent::ReceivedRequest(data, _) = event
                && data == b"this should be muted reqresp"
            {
                panic!("Received muted request - muting failed!");
            }
        }
        Ok(None) => info!("No event received - peer is correctly muted"),
    }

    sleep(Duration::from_secs(4)).await;

    user0[0]
        .reqresp
        .send(RequestResponseCommand {
            #[cfg(feature = "byos")]
            target_app_public_key: target_public_key.clone(),
            #[cfg(not(feature = "byos"))]
            target_transport_id,
            data: "after mute expired".into(),
        })
        .await?;

    if let Some(ReqRespEvent::ReceivedRequest(data, _)) = user1[0].reqresp.next_event().await {
        assert_eq!(data, b"after mute expired");
        info!("Request sent successfully after mute expired");
    } else {
        panic!("Expected ReceivedRequest");
    }

    cancel.cancel();
    tasks.wait().await;
    Ok(())
}
