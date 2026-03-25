//! Tests for new operator functionality.

#[cfg(feature = "byos")]
use std::sync::Arc;
use std::time::Duration;

use futures::SinkExt;
use libp2p::{Multiaddr, build_multiaddr, connection_limits::ConnectionLimits, identity::Keypair};
use tokio::{sync::oneshot::channel, time::sleep};
use tracing::{debug, info};

use super::common::Setup;
#[cfg(feature = "byos")]
use crate::tests::common::MockApplicationSigner;
#[cfg(feature = "mem-conn-limits-abs")]
use crate::tests::common::SIXTEEN_GIBIBYTES;
#[cfg(not(feature = "byos"))]
use crate::validator::DefaultP2PValidator;
use crate::{
    commands::{Command, GossipCommand, QueryP2PStateCommand},
    events::GossipEvent,
    tests::common::{User, init_tracing},
};

/// Starts with an all-to-all mesh of existing operators, then introduces one
/// additional operator and checks that gossip flows in both directions after
/// the new node joins.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn gossip_new_user() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 9;

    // Create the transport identity for the node that will join after the
    // original mesh is already established.
    let new_user_transport_keypair = Keypair::generate_ed25519();
    #[cfg(feature = "byos")]
    // BYOS also needs a separate application identity for the joining node.
    let new_user_app_keypair = Keypair::generate_ed25519();

    info!(
        users = USERS_NUM,
        "Setting up users in all-to-all topology with new user in allowlist"
    );
    // Build the initial mesh with the future newcomer already present in every
    // existing node's allowlist. This keeps the test symmetric once transport
    // allowlist enforcement is enabled.
    #[cfg(feature = "byos")]
    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_with_new_user_allowlist(USERS_NUM, &new_user_app_keypair.public())
        .await?;

    #[cfg(not(feature = "byos"))]
    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_with_new_user_allowlist(
        USERS_NUM,
        &new_user_transport_keypair.public().to_peer_id(),
    )
    .await?;

    // Collect the existing nodes' listening addresses so the late-joining node
    // can proactively dial the already-formed mesh.
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

    // The joining node listens on its own address rather than reusing any of
    // the bootstrapped nodes' addresses.
    let local_addr = build_multiaddr!(Memory(88_888_888_u64));

    // Mirror the allowlist on the newcomer: it should accept every existing
    // node, rather than relying on an open transport policy in this test.
    info!(%local_addr, "Creating new user to listen");
    #[cfg(feature = "byos")]
    let new_user_allowlist = user_handles
        .iter()
        .map(|handle| handle.app_keypair.public())
        .collect::<Vec<_>>();
    #[cfg(not(feature = "byos"))]
    let new_user_allowlist = user_handles
        .iter()
        .map(|handle| handle.peer_id)
        .collect::<Vec<_>>();

    let mut new_user = User::new(
        #[cfg(feature = "byos")]
        new_user_app_keypair.clone(),
        new_user_transport_keypair.clone(),
        connect_addrs.clone(), // Connect directly to existing users
        #[cfg(not(feature = "byos"))]
        Some(new_user_allowlist),
        #[cfg(feature = "byos")]
        new_user_allowlist, // Allow all existing users
        vec![local_addr.clone()],
        cancel.child_token(),
        #[cfg(feature = "byos")]
        Arc::new(MockApplicationSigner::new(new_user_app_keypair.clone())),
        #[cfg(not(feature = "byos"))]
        Box::new(DefaultP2PValidator),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0, // 100 %
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
    )?;

    // Let the new node both dial the old mesh and then remain online to
    // receive the follow-up gossip assertions below.
    tasks.spawn(async move {
        info!("New user is establishing connections");
        new_user.p2p.establish_connections().await;
        info!("New user connections established");

        new_user.p2p.listen().await;
    });

    // Wait for existing users to fully initialize
    sleep(Duration::from_secs(5)).await;

    // Ask each existing node to dial the newcomer as well. By the time this
    // runs some peers may already be connected, which is fine; the test cares
    // that the topology converges, not which side completed the dial first.
    for (index, addr) in connect_addrs.iter().enumerate() {
        info!(
            index,
            addr = %addr,
            old_user_tid = %user_handles[index].peer_id,
            new_user_tid = %new_user.transport_keypair.public().to_peer_id(),
            "Old user connecting to new user"
        );
        #[cfg(feature = "byos")]
        let app_public_key = user_handles[index].app_keypair.public();
        #[cfg(not(feature = "byos"))]
        // Non-BYOS identifies the newcomer by transport peer id.
        let transport_id = new_user.transport_keypair.public().to_peer_id();
        user_handles[index]
            .command
            .send_command(Command::ConnectToPeer {
                #[cfg(feature = "byos")]
                app_public_key,
                #[cfg(not(feature = "byos"))]
                transport_id,
                addresses: vec![local_addr.clone()],
            })
            .await;
    }

    // Give time for the new user to establish connections
    sleep(Duration::from_secs(3)).await;

    // We send one message from the original mesh and one from the newcomer to
    // verify both ingress and egress after the late join.
    let message_from_inside = b"Hello my friends!".to_vec();
    let message_from_outsider = b"Hi, I'm new here.".to_vec();

    info!("Regular user sending test message");
    user_handles[0]
        .gossip
        .send(GossipCommand {
            data: message_from_inside.clone(),
        })
        .await
        .expect("Failed to send gossip message");

    // Wait for message propagation and verify
    sleep(Duration::from_secs(2)).await;

    info!("New user sending test message");
    new_user
        .gossip
        .send(GossipCommand {
            data: message_from_outsider.clone(),
        })
        .await
        .expect("Failed to send gossip message");

    // Wait for message propagation
    sleep(Duration::from_secs(2)).await;

    let mut counter_messages_from_regular_user = 0;
    let mut counter_messages_from_outsider = 0;

    // Existing users should observe both broadcasts: one from an original
    // member and one from the newly joined member.
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

    // The original message should reach the newcomer once, and the newcomer's
    // message should reach each original peer once.
    assert_eq!(counter_messages_from_regular_user, USERS_NUM);
    assert_eq!(counter_messages_from_outsider, USERS_NUM);

    // Clean up
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
