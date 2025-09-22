use std::time::Duration;

use anyhow::bail;
use futures::{SinkExt, StreamExt};
use libp2p::{Multiaddr, identity::Keypair};
use tokio::{sync::oneshot::channel, task, time::sleep};
use tracing::{debug, info};

use super::common::Setup;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    events::CommandEvent,
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
        Ok(CommandEvent::ResultFindMultiaddress(opt)) => match opt {
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

    info!(users = USERS_NUM, "Setting up users in all-to-all topology");

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    info!("Getting listening addresses of first user");
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

    info!("Sending command Command::FindMultiaddr to last user");

    let _ = user_handles[USERS_NUM - 1]
        .command
        .send(Command::FindMultiaddr {
            #[cfg(feature = "byos")]
            app_public_key: Keypair::generate_ed25519().public(),
            #[cfg(not(feature = "byos"))]
            transport_id: Keypair::generate_ed25519().public().to_peer_id(),
        })
        .await;

    info!("Waiting for result from command Command::FindMultiaddr");

    // Choose which version to use:
    // Option 1: Using next_event() - async method approach
    //wait_for_result_with_next_event(user_handles).await?;

    // Option 2: Using next() - Stream API approach (uncomment to use)
    wait_for_result_with_stream_next(user_handles).await?;

    sleep(Duration::from_secs(5)).await;

    // info!(event = ?user_handles[USERS_NUM-1].command.next_event().await);

    cancel.cancel();
    tasks.wait().await;

    Ok(())
}

/// Version using next_event() - async method approach
async fn wait_for_result_with_next_event(
    user_handles: Vec<super::common::UserHandle>,
) -> anyhow::Result<()> {
    const USERS_NUM: usize = 9;
    task::spawn(async move {
        let mut user_handles = user_handles;
        loop {
            tokio::select!(
                result = user_handles[USERS_NUM - 1].command.next_event() => {
                    match result {
                        Ok(CommandEvent::ResultFindMultiaddress(opt)) => {
                            if opt.is_some() {
                                bail!("Somehow, an address for such peer has been found.");
                            }
                            info!("SUCCESS with next_event()!!!");
                            break Ok(());
                        }
                        Ok(_) => continue, // Other CommandEvent variants
                        Err(e) => {
                            bail!("Error while waiting for event from command handler: {}", e);
                        }
                    }
                }
                else => {
                    continue;
                }
            )
        }
    })
    .await?
}

/// Version using next() - Stream API approach
#[allow(dead_code)]
async fn wait_for_result_with_stream_next(
    user_handles: Vec<super::common::UserHandle>,
) -> anyhow::Result<()> {
    const USERS_NUM: usize = 9;
    task::spawn(async move {
        let mut user_handles = user_handles;
        loop {
            tokio::select!(
                Some(result) = user_handles[USERS_NUM - 1].command.next() => {
                    match result {
                        Ok(CommandEvent::ResultFindMultiaddress(opt)) => {
                            if opt.is_some() {
                                bail!("Somehow, an address for such peer has been found.");
                            }
                            info!("SUCCESS with Stream::next()!!!");
                            break Ok(());
                        }
                        Ok(_) => continue, // Other CommandEvent variants
                        Err(e) => {
                            bail!("Error while waiting for event from command handler: {}", e);
                        }
                    }
                }
                else => {
                    continue;
                }
            )
        }
    })
    .await?
}
