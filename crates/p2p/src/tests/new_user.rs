//! Connect to Peer Command tests.

use std::time::Duration;

use anyhow::bail;
use libp2p::{Multiaddr, build_multiaddr, identity::Keypair};
use tokio::{sync::oneshot::channel, time::sleep};
use tracing::{debug, info};
use tracing_test::traced_test;

use super::common::Setup;
use crate::{
    commands::{BanUnbanCommand, Command, ConnectToPeerCommand, QueryP2PStateCommand},
    events::Event,
    tests::common::User,
};

/// Tests sending a gossipsub message from a new user to all existing users.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[traced_test]
async fn gossip_new_user() -> anyhow::Result<()> {
    const USERS_NUM: usize = 9;

    info!("Setting up {} users setup all-to-all", USERS_NUM);
    // Create the original users
    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM, 200).await?;

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

    // Generate a keypair for the new user
    let new_user_keypair = Keypair::generate_ed25519();

    // Create new user with all necessary information
    info!(
        "Creating new user to listen at {} peer_id: {}",
        local_addr,
        new_user_keypair.public().to_peer_id()
    );
    let mut new_user = User::new(
        new_user_keypair.clone(),
        Vec::new(), // doesn't have a blacklist
        Vec::new(), // doesn't know other nodes
        local_addr.clone(),
        cancel.child_token(),
    )
    .unwrap();

    // Run the new user in a separate task - this call will handle connections
    tasks.spawn(async move {
        // This will attempt to establish the connections to other users and subscribe to a topic
        info!("New user is establishing connections");
        new_user.p2p.establish_connections().await;
        info!("New user connections established");

        // This will start listening for messages
        new_user.p2p.listen().await;
    });

    // Create a separate listening address for the new user
    let local_addr2 = build_multiaddr!(Memory(88888889_u64));

    // Generate a keypair for the new user
    let new_user2_keypair = Keypair::generate_ed25519();

    // Create new user with all necessary information
    info!(
        "Creating second new user to listen at {} peer_id: {}",
        local_addr2,
        new_user2_keypair.public().to_peer_id()
    );
    let mut new_user2 = User::new(
        new_user2_keypair.clone(),
        Vec::new(), // doesn't have a blacklist
        Vec::new(), // doesn't know other nodes
        local_addr2.clone(),
        cancel.child_token(),
    )
    .unwrap();

    // Run the second new user in a separate task - this call will handle connections
    tasks.spawn(async move {
        // This will attempt to establish the connections to other users and subscribe to a topic
        info!("Second new user is establishing connections");
        new_user2.p2p.establish_connections().await;
        info!("Second new user connections established");

        // This will start listening for messages
        new_user2.p2p.listen().await;
    });

    // Create a separate listening address for the new user
    let local_addr3 = build_multiaddr!(Memory(88888890_u64));

    // Generate a keypair for the new user
    let new_user3_keypair = Keypair::generate_ed25519();

    // Create new user with all necessary information
    info!(
        "Creating third new user to listen at {} peer_id: {}",
        local_addr3,
        new_user3_keypair.public().to_peer_id()
    );
    let mut new_user3 = User::new(
        new_user3_keypair.clone(),
        Vec::new(), // doesn't have a blacklist
        Vec::new(), // doesn't know other nodes
        local_addr3.clone(),
        cancel.child_token(),
    )
    .unwrap();

    // Run the third new user in a separate task - this call will handle connections
    tasks.spawn(async move {
        // This will attempt to establish the connections to other users and subscribe to a topic
        info!("Third new user is establishing connections");
        new_user3.p2p.establish_connections().await;
        info!("Third new user connections established");

        // This will start listening for messages
        new_user3.p2p.listen().await;
    });

    // Ask old users to ban third new user
    for index in 0..connect_addrs.len() {
        info!(
            "Asking an old user (index {}, connect_addr: {}, peer_id {}) to ban to third new user",
            index, connect_addrs[index], user_handles[index].peer_id
        );
        user_handles[index]
            .handle
            .send_command(Command::BanUnbanCommand {
                peer_id: new_user3.kp.public().to_peer_id(),
                peer_addr: local_addr3.clone(),
                ban_unban: BanUnbanCommand::Ban,
            })
            .await;
    }

    // Ask old users to connect to third new user and therefore remove from their blacklist
    for index in 0..connect_addrs.len() {
        info!(
            "Asking an old user (index {}, connect_addr: {}, peer_id {}) to connect to first new user",
            index, connect_addrs[index], user_handles[index].peer_id
        );
        user_handles[index]
            .handle
            .send_command(Command::ConnectToPeer(ConnectToPeerCommand {
                peer_id: new_user3.kp.public().to_peer_id(),
                peer_addr: local_addr3.clone(),
            }))
            .await;
    }

    // Ask the old users to the first new one
    for index in 0..connect_addrs.len() {
        info!(
            "Asking an old user (index {}, connect_addr: {}, peer_id {}) to connect to first new user",
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

    //  Ask the second new user to connect to old ones
    for index in 0..connect_addrs.len() {
        info!(
            "Asking second new user to connect to an old user (index {}, connect_addr: {}, peer_id {})",
            index, connect_addrs[index], user_handles[index].peer_id
        );
        new_user2
            .handle
            .send_command(Command::ConnectToPeer(ConnectToPeerCommand {
                peer_id: user_handles[index].peer_id,
                peer_addr: connect_addrs[index].clone(),
            }))
            .await;
    }

    // we maybe should also try connect second user to first user, but gossipsub has to fix it by
    // itself.

    // Give time for the new users to establish connections
    sleep(Duration::from_secs(3)).await;

    let message_from_inside = Vec::<u8>::from(b"Hello my friends!");
    let message_from_outsider1 = Vec::<u8>::from(b"Hi, I'm new here.");
    let message_from_outsider2 = Vec::<u8>::from(b"Hello there");
    let message_from_outsider3 = Vec::<u8>::from(b"Hi there");

    info!("Regular user sending test message");
    user_handles[0]
        .handle
        .send_command(Command::PublishMessage {
            data: message_from_inside.clone(),
        })
        .await;

    info!("First new user sending test message");
    new_user
        .handle
        .send_command(Command::PublishMessage {
            data: message_from_outsider1.clone(),
        })
        .await;

    info!("Second new user sending test message");
    new_user2
        .handle
        .send_command(Command::PublishMessage {
            data: message_from_outsider2.clone(),
        })
        .await;

    info!("Third new user sending test message");
    new_user3
        .handle
        .send_command(Command::PublishMessage {
            data: message_from_outsider3.clone(),
        })
        .await;

    // Wait for message propagation
    sleep(Duration::from_secs(2)).await;

    let mut counter_messages_from_regular_user = 0;
    let mut counter_messages_from_outsider1 = 0;
    let mut counter_messages_from_outsider2 = 0;
    let mut counter_messages_from_outsider3 = 0;

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
                    } else if msg == message_from_outsider1 {
                        info!("User received message from first new user");
                        counter_messages_from_outsider1 += 1;
                    } else if msg == message_from_outsider2 {
                        info!("User received message from second new user");
                        counter_messages_from_outsider2 += 1;
                    } else if msg == message_from_outsider3 {
                        info!("User received message from third new user");
                        counter_messages_from_outsider3 += 1;
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
                    info!("First new user received message from regular user");
                    counter_messages_from_regular_user += 1;
                } else if msg == message_from_outsider1 {
                    info!("First new user received message from first new user");
                    counter_messages_from_outsider1 += 1;
                } else if msg == message_from_outsider2 {
                    info!("First new user received message from second new user");
                    counter_messages_from_outsider2 += 1;
                } else if msg == message_from_outsider3 {
                    info!("First new user received message from third new user");
                    counter_messages_from_outsider3 += 1;
                }
            }
            _ => bail!("Unexpected event type"),
        }
    }

    while !new_user2.handle.events_is_empty() {
        let event = new_user2.handle.next_event().await?;
        info!(?event, "Received event");

        match event {
            Event::ReceivedMessage(msg) => {
                if msg == message_from_inside {
                    info!("Second new user received message from regular user");
                    counter_messages_from_regular_user += 1;
                } else if msg == message_from_outsider1 {
                    info!("Second new user received message from first new user");
                    counter_messages_from_outsider1 += 1;
                } else if msg == message_from_outsider2 {
                    info!("Second new user received message from second new user");
                    counter_messages_from_outsider2 += 1;
                } else if msg == message_from_outsider3 {
                    info!("Second new user received message from third new user");
                    counter_messages_from_outsider3 += 1;
                }
            }
            _ => bail!("Unexpected event type"),
        }
    }

    while !new_user3.handle.events_is_empty() {
        let event = new_user3.handle.next_event().await?;
        info!(?event, "Received event");

        match event {
            Event::ReceivedMessage(msg) => {
                if msg == message_from_inside {
                    info!("Third new user received message from regular user");
                    counter_messages_from_regular_user += 1;
                } else if msg == message_from_outsider1 {
                    info!("Third new user received message from first new user");
                    counter_messages_from_outsider1 += 1;
                } else if msg == message_from_outsider2 {
                    info!("Third new user received message from second new user");
                    counter_messages_from_outsider2 += 1;
                } else if msg == message_from_outsider3 {
                    info!("Third new user received message from third new user");
                    counter_messages_from_outsider3 += 1;
                }
            }
            _ => bail!("Unexpected event type"),
        }
    }

    assert_eq!(
        (
            counter_messages_from_regular_user,
            counter_messages_from_outsider1,
            counter_messages_from_outsider2,
            counter_messages_from_outsider3
        ),
        (USERS_NUM + 2, USERS_NUM + 2, USERS_NUM + 2, USERS_NUM + 2),
        "messages from old users ; messages from first new user ; messages from second new user ; messages from third new user"
    );
    // Clean up
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
