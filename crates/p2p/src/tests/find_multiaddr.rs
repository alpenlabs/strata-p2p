use anyhow::bail;
use futures::StreamExt;
use libp2p::{Multiaddr, identity::Keypair};
use tokio::sync::oneshot::channel;
use tracing::{debug, info};

use super::common::Setup;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    events::CommandEvents,
    tests::common::init_tracing,
};

// Test in which we test command [`Command::FindMultiaddr`].
#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_find_multiaddr() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 9;

    info!(
        users = USERS_NUM,
        "Setting up users in all-to-all topology with new user in allowlist"
    );

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

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

    info!("Sending command Command::FindMultiaddr to last old user");

    user_handles[USERS_NUM - 1]
        .command
        .send_command(Command::FindMultiaddr {
            #[cfg(feature = "byos")]
            app_public_key: user_handles[0].app_keypair.public(),
            #[cfg(not(feature = "byos"))]
            transport_id: user_handles[0].peer_id,
        })
        .await;

    info!("Waiting for result from command Command::FindMultiaddr");

    match user_handles[USERS_NUM - 1].command.next_event().await {
        Ok(CommandEvents::ResultFindMultiaddress(opt)) => match opt {
            Some(vec_addrs) => {
                assert!(vec_addrs.iter().any(|x| x == connect_addr));
            }
            None => {
                bail!("Unfortunately, no address for such peer has been found.");
            }
        },
        Err(e) => {
            bail!("Error while waiting for event from command handler: {}", e);
        }
    };

    // Clean up
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}

// Test in which we test command [`Command::FindMultiaddr`].
#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_find_non_existent_multiaddr() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 9;

    info!(
        users = USERS_NUM,
        "Setting up users in all-to-all topology with new user in allowlist"
    );

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

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

    info!("Sending command Command::FindMultiaddr to last old user");

    user_handles[USERS_NUM - 1]
        .command
        .send_command(Command::FindMultiaddr {
            #[cfg(feature = "byos")]
            app_public_key: Keypair::generate_ed25519().public(),
            #[cfg(not(feature = "byos"))]
            transport_id: Keypair::generate_ed25519().public().to_peer_id(),
        })
        .await;

    info!("Waiting for result from command Command::FindMultiaddr");

    while let Some(smth) = user_handles[USERS_NUM - 1].command.next().await {
        match smth {
            Ok(CommandEvents::ResultFindMultiaddress(opt)) => {
                if opt.is_some() {
                    bail!("Somehow, an address for such peer has been found.");
                } else {
                    break;
                }
            }
            Err(e) => {
                bail!("Error while waiting for event from command handler: {}", e);
            }
        }
    }

    // Clean up
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
