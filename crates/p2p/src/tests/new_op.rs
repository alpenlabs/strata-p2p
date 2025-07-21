//! Connect to Peer Command tests.

use std::{collections::HashSet, time::Duration};

use libp2p::{Multiaddr, build_multiaddr, identity::Keypair};
use tokio::{sync::oneshot::channel, time::sleep};
use tracing::{debug, info};

use super::common::Setup;
use crate::{
    commands::{Command, ConnectToPeerCommand, QueryP2PStateCommand},
    events::GossipEvent,
    swarm::setup::behavior::BanList,
    tests::common::{User, init_tracing},
};

/// Tests sending a gossipsub message from a new user to all existing users.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn gossip_new_user() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 9;

    // Generate a keypair for the new user
    let new_user_app_keypair = Keypair::generate_ed25519();

    info!(users = USERS_NUM, "Setting up users in all-to-all topology");
    // Create the original users with allowlist containing the new user
    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

    // Get connection addresses of old users for the new user to connect to.
    info!("Getting listening addresses for new user");
    let mut connect_addrs = Vec::with_capacity(USERS_NUM);
    for (index, user_handle) in user_handles.iter().enumerate().take(USERS_NUM) {
        let (tx, rx) = channel::<Vec<Multiaddr>>();
        user_handle
            .command
            .send_command(Command::QueryP2PState(
                QueryP2PStateCommand::GetMyListeningAddresses {
                    response_sender: tx,
                },
            ))
            .await;
        let result = rx.await.unwrap();
        debug!(index, addresses = ?result, "Retrieved listening addresses");
        connect_addrs.push(result[0].clone());
    }

    // Create a separate listening address for the new user
    let local_addr = build_multiaddr!(Memory(88888888_u64));

    // Create new user with all necessary information
    info!(%local_addr, "Creating new user to listen");
    let new_user_transport_keypair = Keypair::generate_ed25519();
    let mut new_user = User::new(
        new_user_app_keypair.clone(),
        new_user_transport_keypair.clone(),
        connect_addrs.clone(), // Connect directly to existing users
        local_addr.clone(),
        BanList::new(HashSet::new()),
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
        user_handles[index]
            .command
            .send_command(Command::ConnectToPeer(ConnectToPeerCommand {
                peer_id: new_user.transport_keypair.public().to_peer_id(),
                peer_addr: local_addr.clone(),
            }))
            .await;
    }

    // Give time for the new user to establish connections
    sleep(Duration::from_secs(5)).await;

    let message_from_inside = b"Hello my friends!".to_vec();
    let message_from_outsider = b"Hi, I'm new here.".to_vec();

    info!("Regular user sending test message");
    user_handles[0]
        .command
        .send_command(Command::PublishMessage {
            data: message_from_inside.clone(),
        })
        .await;

    // Wait for message propagation and verify
    sleep(Duration::from_secs(2)).await;

    info!("New user sending test message");
    new_user
        .command
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
        info!(peer_id = %user.peer_id, "Checking if user received message");

        while !user.gossip.events_is_empty() {
            let event = user.gossip.next_event().await?;
            debug!(?event, "Received event");

            match event {
                GossipEvent::ReceivedMessage(msg) => {
                    if msg == message_from_inside {
                        info!("User received message from regular user");
                        counter_messages_from_regular_user += 1;
                    } else if msg == message_from_outsider {
                        info!("User received message from new user");
                        counter_messages_from_outsider += 1;
                    }
                }
            }
        }
    }
    while !new_user.gossip.events_is_empty() {
        let event = new_user.gossip.next_event().await?;
        debug!(?event, "New user received event");

        match event {
            GossipEvent::ReceivedMessage(msg) => {
                if msg == message_from_inside {
                    info!("New user received message from regular user");
                    counter_messages_from_regular_user += 1;
                } else if msg == message_from_outsider {
                    info!("New user received message from new user");
                    counter_messages_from_outsider += 1;
                }
            }
        }
    }

    assert_eq!(counter_messages_from_regular_user, USERS_NUM);
    assert_eq!(counter_messages_from_outsider, USERS_NUM);

    // Clean up
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
