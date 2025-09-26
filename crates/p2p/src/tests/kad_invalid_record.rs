use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use anyhow::bail;
use futures::SinkExt;
use libp2p::{
    Multiaddr, StreamProtocol, Transport, build_multiaddr,
    bytes::BufMut,
    core::{muxing::StreamMuxerBox, transport::MemoryTransport},
    identify::{Behaviour as Identify, Config},
    identity,
    kad::{self, Event as KadEvent},
    noise,
    swarm::{NetworkBehaviour, SwarmEvent, dial_opts::DialOpts},
    yamux,
};
use tokio::sync::oneshot::{self, channel};
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tracing::{debug, info};

use super::common::Setup;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    events::CommandEvent,
    signer::TransportKeypairSigner,
    swarm::{
        PROTOCOL_NAME,
        message::dht_record::{RecordData, SignedRecord},
    },
    tests::common::init_tracing,
};

/// Composite behaviour which consists of other ones used by swarm in P2P
/// implementation.
#[derive(NetworkBehaviour)]
struct Behaviour {
    /// Identification of peers, address to connect to, public keys, etc.
    pub identify: Identify,

    /// Kademlia DHT
    pub kademlia: kad::Behaviour<kad::store::MemoryStore>,
}

// Test in which we test command [`Command::FindMultiaddr`].
#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_do_not_find_invalid_record() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 9;

    info!(users = USERS_NUM, "Setting up users in all-to-all topology");

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

    let mut peer_ids = Vec::with_capacity(USERS_NUM);
    for user_handle in &user_handles {
        peer_ids.push(user_handle.peer_id);
    }

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let local_key = identity::Keypair::generate_ed25519();

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_other_transport(|our_keypair| {
            MemoryTransport::new()
                .upgrade(libp2p::core::upgrade::Version::V1)
                .authenticate(noise::Config::new(our_keypair).unwrap())
                .multiplex(yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        })?
        .with_behaviour(|key| {
            let mut cfg = kad::Config::new(StreamProtocol::new("/kad/strata/0.0.1"));
            cfg.set_query_timeout(Duration::from_secs(5 * 60));
            let store = kad::store::MemoryStore::new(key.public().to_peer_id());
            let kad = kad::Behaviour::with_config(key.public().to_peer_id(), store, cfg);
            Behaviour {
                kademlia: kad,
                identify: Identify::new(Config::new(PROTOCOL_NAME.to_string(), local_key.public())),
            }
        })?
        .build();

    swarm.listen_on(build_multiaddr!(Memory(100000u64)))?;

    tokio::spawn(async move {
        use futures::StreamExt;

        info!(?connect_addrs, "Connection addresses...");

        let mut flag: bool = false;

        loop {
            let event = swarm.select_next_some().await;
            info!(?event, "received event");
            match event {
                SwarmEvent::NewListenAddr {
                    listener_id: _,
                    address: _,
                } => {
                    info!("DIALING");

                    connect_addrs
                        .iter()
                        .zip(peer_ids.clone())
                        .for_each(|(addr, peer_id)| {
                            info!(%addr, %peer_id, "HELLO");
                            swarm
                                .behaviour_mut()
                                .kademlia
                                .add_address(&peer_id, addr.clone());
                            swarm
                                .dial(
                                    DialOpts::peer_id(peer_id)
                                        .addresses(vec![addr.clone()])
                                        .condition(libp2p::swarm::dial_opts::PeerCondition::Always)
                                        .build(),
                                )
                                .unwrap()
                        });
                }
                SwarmEvent::Behaviour(BehaviourEvent::Kademlia(KadEvent::RoutingUpdated {
                    peer: _,
                    is_new_peer: _,
                    addresses: _,
                    bucket_range: _,
                    old_peer: _,
                })) => {
                    if !flag {
                        info!("PUTTING RECORD");

                        let mut pk_record_key = vec![];
                        pk_record_key.put_slice(swarm.local_peer_id().to_bytes().as_slice());

                        let mut pk_record =
                            kad::Record::new(pk_record_key, "\0".as_bytes().to_vec());
                        pk_record.publisher = Some(*swarm.local_peer_id());
                        pk_record.expires = Some(Instant::now() + Duration::from_secs(60));

                        swarm
                            .behaviour_mut()
                            .kademlia
                            .put_record(pk_record, kad::Quorum::N(NonZeroUsize::new(3).unwrap()))
                            .unwrap();

                        flag = true;
                    }
                }
                _ => {}
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    let (tx, rx) = oneshot::channel();

    user_handles[0]
        .command
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetConnectedPeers {
                response_sender: tx,
            },
        ))
        .await;

    match rx.await {
        Ok(v) => assert_eq!(v.len(), USERS_NUM),
        Err(e) => bail!("error {e}"),
    };

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    info!("Sending command Command::FindMultiaddr to last user");

    let _ = user_handles[USERS_NUM - 1]
        .command
        .send(Command::FindMultiaddr {
            transport_id: local_key.public().to_peer_id(),
        })
        .await;

    match BroadcastStream::new(user_handles[USERS_NUM - 1].command.get_new_receiver())
        .next()
        .await
    {
        Some(Ok(CommandEvent::ResultFindMultiaddress(opt))) => {
            if opt.is_some() {
                bail!("Somehow, an address for such peer has been found.");
            }
        }
        Some(Err(e)) => {
            bail!("Error while waiting for event from command handler: {}", e);
        }
        None => {
            bail!("Error while waiting for event from command handler: we got None.");
        }
    }

    // Clean up
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}

// Test in which we test command [`Command::FindMultiaddr`].
#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_do_not_find_empty_record() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 9;

    info!(users = USERS_NUM, "Setting up users in all-to-all topology");

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

    let mut peer_ids = Vec::with_capacity(USERS_NUM);
    for user_handle in &user_handles {
        peer_ids.push(user_handle.peer_id);
    }

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let local_key = identity::Keypair::generate_ed25519();

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_other_transport(|our_keypair| {
            MemoryTransport::new()
                .upgrade(libp2p::core::upgrade::Version::V1)
                .authenticate(noise::Config::new(our_keypair).unwrap())
                .multiplex(yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        })?
        .with_behaviour(|key| {
            let mut cfg = kad::Config::new(StreamProtocol::new("/kad/strata/0.0.1"));
            cfg.set_query_timeout(Duration::from_secs(5 * 60));
            let store = kad::store::MemoryStore::new(key.public().to_peer_id());
            let kad = kad::Behaviour::with_config(key.public().to_peer_id(), store, cfg);
            Behaviour {
                kademlia: kad,
                identify: Identify::new(Config::new(PROTOCOL_NAME.to_string(), local_key.public())),
            }
        })?
        .build();

    swarm.listen_on(build_multiaddr!(Memory(100000u64)))?;

    let copy_local_key = local_key.clone();

    tokio::spawn(async move {
        use futures::StreamExt;

        info!(?connect_addrs, "Connection addresses...");

        let mut flag: bool = false;

        loop {
            let event = swarm.select_next_some().await;
            info!(?event, "received event");
            match event {
                SwarmEvent::NewListenAddr {
                    listener_id: _,
                    address: _,
                } => {
                    info!("DIALING");

                    connect_addrs
                        .iter()
                        .zip(peer_ids.clone())
                        .for_each(|(addr, peer_id)| {
                            info!(%addr, %peer_id, "HELLO");
                            swarm
                                .behaviour_mut()
                                .kademlia
                                .add_address(&peer_id, addr.clone());
                            swarm
                                .dial(
                                    DialOpts::peer_id(peer_id)
                                        .addresses(vec![addr.clone()])
                                        .condition(libp2p::swarm::dial_opts::PeerCondition::Always)
                                        .build(),
                                )
                                .unwrap()
                        });
                }
                SwarmEvent::Behaviour(BehaviourEvent::Kademlia(KadEvent::RoutingUpdated {
                    peer: _,
                    is_new_peer: _,
                    addresses: _,
                    bucket_range: _,
                    old_peer: _,
                })) => {
                    if !flag {
                        info!("PUTTING RECORD");
                        let mut pk_record_key = vec![];
                        pk_record_key.put_slice(swarm.local_peer_id().to_bytes().as_slice());

                        let mut pk_record = kad::Record::new(
                            pk_record_key,
                            copy_local_key.public().encode_protobuf(),
                        );
                        pk_record.publisher = Some(*swarm.local_peer_id());
                        pk_record.expires = Some(Instant::now() + Duration::from_secs(60));

                        swarm
                            .behaviour_mut()
                            .kademlia
                            .put_record(pk_record, kad::Quorum::N(NonZeroUsize::new(3).unwrap()))
                            .unwrap();

                        flag = true;
                    }
                }
                _ => {}
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    let (tx, rx) = oneshot::channel();

    user_handles[0]
        .command
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetConnectedPeers {
                response_sender: tx,
            },
        ))
        .await;

    match rx.await {
        Ok(v) => assert_eq!(v.len(), USERS_NUM),
        Err(e) => bail!("error {e}"),
    };

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    info!("Sending command Command::FindMultiaddr to last user");

    let _ = user_handles[USERS_NUM - 1]
        .command
        .send(Command::FindMultiaddr {
            transport_id: local_key.public().to_peer_id(),
        })
        .await;

    match BroadcastStream::new(user_handles[USERS_NUM - 1].command.get_new_receiver())
        .next()
        .await
    {
        Some(Ok(CommandEvent::ResultFindMultiaddress(opt))) => {
            if opt.is_some() {
                bail!("Somehow, an address for such peer has been found.");
            }
        }
        Some(Err(e)) => {
            bail!("Error while waiting for event from command handler: {}", e);
        }
        None => {
            bail!("Error while waiting for event from command handler: we got None.");
        }
    }

    // Clean up
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}

// Test in which we test command [`Command::FindMultiaddr`].
#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_do_not_find_record_with_not_corresponding_key() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 9;

    info!(users = USERS_NUM, "Setting up users in all-to-all topology");

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

    let mut peer_ids = Vec::with_capacity(USERS_NUM);
    for user_handle in &user_handles {
        peer_ids.push(user_handle.peer_id);
    }

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let local_key = identity::Keypair::generate_ed25519();

    let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key.clone())
        .with_tokio()
        .with_other_transport(|our_keypair| {
            MemoryTransport::new()
                .upgrade(libp2p::core::upgrade::Version::V1)
                .authenticate(noise::Config::new(our_keypair).unwrap())
                .multiplex(yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        })?
        .with_behaviour(|key| {
            let mut cfg = kad::Config::new(StreamProtocol::new("/kad/strata/0.0.1"));
            cfg.set_query_timeout(Duration::from_secs(5 * 60));
            let store = kad::store::MemoryStore::new(key.public().to_peer_id());
            let kad = kad::Behaviour::with_config(key.public().to_peer_id(), store, cfg);
            Behaviour {
                kademlia: kad,
                identify: Identify::new(Config::new(PROTOCOL_NAME.to_string(), local_key.public())),
            }
        })?
        .build();

    swarm.listen_on(build_multiaddr!(Memory(100000u64)))?;

    let another_keypair = identity::Keypair::generate_ed25519();
    let copy_another_keypair = another_keypair.clone();

    tokio::spawn(async move {
        use futures::StreamExt;

        info!(?connect_addrs, "Connection addresses...");

        let mut flag: bool = false;

        loop {
            let event = swarm.select_next_some().await;
            info!(?event, "received event");
            match event {
                SwarmEvent::NewListenAddr {
                    listener_id: _,
                    address: _,
                } => {
                    info!("DIALING");

                    connect_addrs
                        .iter()
                        .zip(peer_ids.clone())
                        .for_each(|(addr, peer_id)| {
                            info!(%addr, %peer_id, "HELLO");
                            swarm
                                .behaviour_mut()
                                .kademlia
                                .add_address(&peer_id, addr.clone());
                            swarm
                                .dial(
                                    DialOpts::peer_id(peer_id)
                                        .addresses(vec![addr.clone()])
                                        .condition(libp2p::swarm::dial_opts::PeerCondition::Always)
                                        .build(),
                                )
                                .unwrap()
                        });
                }
                SwarmEvent::Behaviour(BehaviourEvent::Kademlia(KadEvent::RoutingUpdated {
                    peer: _,
                    is_new_peer: _,
                    addresses: _,
                    bucket_range: _,
                    old_peer: _,
                })) => {
                    if !flag {
                        info!("PUTTING RECORD");

                        let mut pk_record_key = vec![];
                        pk_record_key
                            .put_slice(another_keypair.public().to_peer_id().to_bytes().as_slice());

                        let mut pk_record = kad::Record::new(
                            pk_record_key,
                            flexbuffers::to_vec(
                                SignedRecord::new(
                                    RecordData::new(
                                        local_key.public(),
                                        swarm.listeners().cloned().collect::<Vec<_>>(),
                                    ),
                                    &TransportKeypairSigner::new(local_key.clone()),
                                )
                                .await
                                .unwrap(),
                            )
                            .unwrap(),
                        );
                        pk_record.publisher = Some(*swarm.local_peer_id());
                        pk_record.expires = Some(Instant::now() + Duration::from_secs(60));

                        swarm
                            .behaviour_mut()
                            .kademlia
                            .put_record(pk_record, kad::Quorum::N(NonZeroUsize::new(3).unwrap()))
                            .unwrap();

                        flag = true;
                    }
                }
                _ => {}
            }
        }
    });

    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    let (tx, rx) = oneshot::channel();

    user_handles[0]
        .command
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetConnectedPeers {
                response_sender: tx,
            },
        ))
        .await;

    match rx.await {
        Ok(v) => assert_eq!(v.len(), USERS_NUM),
        Err(e) => bail!("error {e}"),
    };

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    info!("Sending command Command::FindMultiaddr to last user");

    let _ = user_handles[USERS_NUM - 1]
        .command
        .send(Command::FindMultiaddr {
            transport_id: copy_another_keypair.public().to_peer_id(),
        })
        .await;

    match BroadcastStream::new(user_handles[USERS_NUM - 1].command.get_new_receiver())
        .next()
        .await
    {
        Some(Ok(CommandEvent::ResultFindMultiaddress(opt))) => {
            if opt.is_some() {
                bail!("Somehow, an address for such peer has been found.");
            }
        }
        Some(Err(e)) => {
            bail!("Error while waiting for event from command handler: {}", e);
        }
        None => {
            bail!("Error while waiting for event from command handler: we got None.");
        }
    }

    // Clean up
    cancel.cancel();
    tasks.wait().await;

    Ok(())
}
