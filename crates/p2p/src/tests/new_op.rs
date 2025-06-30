//! Connect to Peer Command tests.

use std::time::Duration;

use anyhow::bail;
use libp2p::{Multiaddr, PeerId, build_multiaddr, identity::Keypair};
use tokio::{sync::oneshot::channel, time::sleep};
use tracing::{debug, info};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use super::common::Setup;
use crate::{
    commands::{Command, ConnectToPeerCommand, QueryP2PStateCommand},
    events::Event,
    tests::common::User,
};

/// Tests sending a gossipsub message from a new user to all existing users.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn gossip_new_user() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer().with_file(true).with_line_number(true))
        .init();

    const USERS_NUM: usize = 9;

    // Generate a keypair for the new user
    let new_user_keypair = Keypair::generate_ed25519();

    info!("Setting up {} users setup all-to-all", USERS_NUM);
    // Create the original users with allowlist containing the new user
    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

    // Create peer IDs of existing users
    let peer_ids: Vec<PeerId> = user_handles.iter().map(|op| op.peer_id).collect();

    // Get connection addresses of old users for the new user to connect to.
    info!("Getting listening addresses for new user");
    let mut connect_addrs = Vec::with_capacity(USERS_NUM);
    for (index, user_handle) in user_handles.iter().enumerate().take(USERS_NUM) {
        let (tx, rx) = channel::<Vec<Multiaddr>>();
        user_handle
            .handle
            .send_command(Command::QueryP2PState(
                QueryP2PStateCommand::GetMyListeningAddresses {
                    response_sender: tx,
                },
            ))
            .await;
        let result = rx.await.unwrap();
        debug!(
            "Got next listening multiaddresses for user {index} : {:?}",
            result
                .iter()
                .map(|addr| addr.to_string())
                .collect::<Vec<_>>()
        );
        connect_addrs.push(result[0].clone());
    }

    // Create a separate listening address for the new user
    let local_addr = build_multiaddr!(Memory(88888888_u64));

    // Create new user with all necessary information
    info!("Creating new user to listen at {}", local_addr);
    let mut new_user = User::new(
        new_user_keypair.clone(),
        peer_ids.clone(),
        connect_addrs.clone(), // Connect directly to existing users
        local_addr.clone(),
        cancel.child_token(),
    )
    .unwrap();

    // Wait for existing users to fully initialize
    sleep(Duration::from_millis(5000)).await;

    // Run the new user in a separate task - this call will handle connections
    tasks.spawn(async move {
        // This will attempt to establish the connections to other users
        info!("New user is establishing connections");
        new_user.p2p.establish_connections().await;
        info!("New user connections established");

        // This will start listening for messages
        new_user.p2p.listen().await;
    });

    // Wait for existing users to fully initialize
    sleep(Duration::from_millis(5000)).await;

    // Connect the old users to the new one
    for index in 0..connect_addrs.len() {
        info!(
            "Asking an old user (index {}, connect_addr: {}, peer_id {}) to connect to the new user",
            index, connect_addrs[index], user_handles[index].peer_id
        );
        user_handles[index]
            .handle
            .send_command(Command::ConnectToPeer(ConnectToPeerCommand {
                peer_id: new_user.kp.public().to_peer_id(),
                peer_addr: local_addr.clone(),
            }))
            .await;
    }

    // Give time for the new user to establish connections
    sleep(Duration::from_secs(5)).await;

    let message_from_inside = Vec::<u8>::from(b"Hello my friends!");
    let message_from_outsider = Vec::<u8>::from(b"Hi, I'm new here.");

    info!("Regular user sending test message");
    user_handles[0]
        .handle
        .send_command(Command::PublishMessage {
            data: message_from_inside.clone(),
        })
        .await;

    // Wait for message propagation and verify
    sleep(Duration::from_secs(2)).await;

    info!("New user sending test message");
    new_user
        .handle
        .send_command(Command::PublishMessage {
            data: message_from_outsider.clone(),
        })
        .await;

    // Wait for message propagation
    sleep(Duration::from_secs(2)).await;

    let mut counter_messages_from_regular_user = 0;
    let mut counter_messages_from_outsider = 0;

    // Check that existing users received the message
    for user in &mut user_handles {
        info!(peer_id=%user.peer_id, "Checking if user received message");

        while !user.handle.events_is_empty() {
            let event = user.handle.next_event().await?;
            info!(?event, "Received event");

            match event {
                Event::ReceivedMessage(msg) => {
                    if msg == message_from_inside {
                        info!("User received message from regular user");
                        counter_messages_from_regular_user += 1;
                    } else if msg == message_from_outsider {
                        info!("User received message from new user");
                        counter_messages_from_outsider += 1;
                    }
                }
                _ => bail!("Unexpected event type"),
            }
        }
    }
    while !new_user.handle.events_is_empty() {
        let event = new_user.handle.next_event().await?;
        info!(?event, "Received event");

        match event {
            Event::ReceivedMessage(msg) => {
                if msg == message_from_inside {
                    info!("New user received message from regular user");
                    counter_messages_from_regular_user += 1;
                } else if msg == message_from_outsider {
                    info!("New user received message from new user");
                    counter_messages_from_outsider += 1;
                }
            }
            _ => bail!("Unexpected event type"),
        }
    }

    assert_eq!(counter_messages_from_regular_user, USERS_NUM);
    assert_eq!(counter_messages_from_outsider, USERS_NUM);

    // Clean up
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
