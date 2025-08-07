#[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use libp2p::{Multiaddr, build_multiaddr, identity::Keypair};
use tokio::{
    sync::oneshot::{self, channel},
    time::{sleep, timeout},
};
use tracing::{debug, info};

use super::common::Setup;
#[cfg(not(feature = "byos"))]
use crate::validator::DefaultP2PValidator;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    tests::common::{MockApplicationSigner, User, init_tracing},
};

/// Test in which a new user connects to one old user, than after sometime is connected to all
/// existing users, because of Kademlia, then tries to send [`Command::DHTGetClosestPeers`]
#[tokio::test(flavor = "multi_thread", worker_threads = 11)]
async fn dht_closestpeers() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 9;

    // Generate a keypair for the new user
    let new_user_app_keypair = Keypair::generate_ed25519();

    info!(
        users = USERS_NUM,
        "Setting up users in all-to-all topology with new user in allowlist"
    );

    // Create the original users with allowlist containing the new user
    #[cfg(feature = "byos")]
    let Setup {
        user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all_with_new_user_allowlist(USERS_NUM, &new_user_app_keypair.public())
        .await?;

    #[cfg(not(feature = "byos"))]
    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

    // Get connection addresses of first old user for the new user to connect to.
    info!("Getting listening addresses for new user");
    let (tx, rx) = channel::<Vec<Multiaddr>>();
    user_handles[0]
        .command
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetMyListeningAddresses {
                response_sender: tx,
            },
        ))
        .await;
    let result = rx.await.unwrap();
    debug!(addresses = ?result, "Retrieved listening addresses");
    let connect_addr = &result[0];

    // Create a separate listening address for the new user
    let local_addr = build_multiaddr!(Memory(88_888_888_u64));

    // Create new user with allowlist of all existing users
    info!(%local_addr, "Creating new user to listen");
    let new_user_transport_keypair = Keypair::generate_ed25519();

    // Create allowlist for new user (all existing users)
    let new_user_allowlist = user_handles
        .iter()
        .map(|handle| handle.app_keypair.public())
        .collect::<Vec<_>>();

    let mut new_user = User::new(
        #[cfg(feature = "byos")]
        new_user_app_keypair.clone(),
        new_user_transport_keypair.clone(),
        vec![connect_addr.clone()], // Connect directly to existing users
        #[cfg(feature = "byos")]
        new_user_allowlist, // Allow all existing users
        vec![local_addr.clone()],
        cancel.child_token(),
        #[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
        Arc::new(MockApplicationSigner::new(new_user_app_keypair.clone())),
        #[cfg(any(
            feature = "gossipsub",
            feature = "request-response",
            not(feature = "byos")
        ))]
        Box::pin(DefaultP2PValidator),
    )?;

    // Run the new user in a separate task - this call will handle connections
    tasks.spawn(async move {
        // This will attempt to establish the connections to other users and subscribe to a topic
        info!("New user is establishing connections");
        new_user.p2p.establish_connections().await;
        info!("New user connections established");

        // This will start listening for messages
        new_user.p2p.listen().await;
    });

    // Ask first old user to connect to the new one
    info!(
        addr = %connect_addr.clone(),
        old_peer = %user_handles[0].peer_id,
        new_peer = %new_user.app_keypair.public().to_peer_id(),
        "Old user connecting to new user"
    );
    let app_public_key = user_handles[0].app_keypair.public();
    user_handles[0]
        .command
        .send_command(Command::ConnectToPeer {
            app_public_key,
            addresses: vec![local_addr.clone()],
        })
        .await;

    // Give time for the new users to establish connections
    sleep(Duration::from_secs(10)).await;

    // Get connection addresses of first old user for the new user to connect to.
    info!("Getting listening addresses for new user");
    let (tx, rx) = channel::<Vec<Multiaddr>>();
    user_handles[USERS_NUM - 1]
        .command
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetMyListeningAddresses {
                response_sender: tx,
            },
        ))
        .await;
    let result = rx.await.unwrap();
    debug!(addresses = ?result, "Retrieved listening addresses");
    let listening_addr_of_last_user = &result[0];

    let (tx, rx) = oneshot::channel();

    info!(
        "Sending command Command::QueryP2PState(QueryP2PStateCommand::GetConnectedPeers) to last old user"
    );

    new_user
        .command
        .send_command(Command::DHTGetClosestPeer {
            transport_id: user_handles[USERS_NUM - 1].peer_id,
            response_sender: tx,
        })
        .await;

    info!("Waiting for result from command Command::DHTGetClosestPeer");

    match timeout(Duration::from_secs(1), rx).await {
        Ok(Ok(v)) => match v {
            Some(addresses) => assert_eq!(
                addresses[0].iter().collect::<Vec<_>>()[0],
                listening_addr_of_last_user
                    .clone()
                    .iter()
                    .collect::<Vec<_>>()[0]
            ),
            None => {
                bail!("We got no result.")
            }
        },
        Ok(Err(e)) => bail!("Error receiving in oneshot channel {e}"),
        Err(e) => bail!("Timeout. {e}"),
    };

    // Clean up
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
