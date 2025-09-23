use std::{
    num::NonZeroUsize,
    time::{Duration, Instant},
};

use anyhow::bail;
use futures::SinkExt;
use libp2p::{
    Multiaddr, StreamProtocol, Transport,
    bytes::BufMut,
    core::{muxing::StreamMuxerBox, transport::MemoryTransport},
    identity, kad, noise, yamux,
};
use tokio::sync::oneshot::channel;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};
use tracing::{debug, info};

use super::common::Setup;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    events::CommandEvent,
    signer::TransportKeypairSigner,
    swarm::message::dht_record::{RecordData, SignedRecord},
    tests::common::init_tracing,
};

// Test in which we test command [`Command::FindMultiaddr`].
#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_do_not_find_invalid_record() -> anyhow::Result<()> {
    init_tracing();
    const USERS_NUM: usize = 9;

    const IPFS_PROTO_NAME: StreamProtocol = StreamProtocol::new("/ipfs/kad/1.0.0");

    info!(users = USERS_NUM, "Setting up users in all-to-all topology");

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

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
            // Create a Kademlia behaviour.
            let mut cfg = kad::Config::new(IPFS_PROTO_NAME);
            cfg.set_query_timeout(Duration::from_secs(5 * 60));
            let store = kad::store::MemoryStore::new(key.public().to_peer_id());
            kad::Behaviour::with_config(key.public().to_peer_id(), store, cfg)
        })?
        .build();

    let mut pk_record_key = vec![];
    pk_record_key.put_slice(swarm.local_peer_id().to_bytes().as_slice());

    let mut pk_record = kad::Record::new(pk_record_key, local_key.public().encode_protobuf());
    pk_record.publisher = Some(*swarm.local_peer_id());
    pk_record.expires = Some(Instant::now() + Duration::from_secs(60));

    swarm
        .behaviour_mut()
        .put_record(pk_record, kad::Quorum::N(NonZeroUsize::new(3).unwrap()))?;

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
            transport_id: local_key.public().to_peer_id(),
        })
        .await;

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

    const IPFS_PROTO_NAME: StreamProtocol = StreamProtocol::new("/ipfs/kad/1.0.0");

    info!(users = USERS_NUM, "Setting up users in all-to-all topology");

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

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
            // Create a Kademlia behaviour.
            let mut cfg = kad::Config::new(IPFS_PROTO_NAME);
            cfg.set_query_timeout(Duration::from_secs(5 * 60));
            let store = kad::store::MemoryStore::new(key.public().to_peer_id());
            kad::Behaviour::with_config(key.public().to_peer_id(), store, cfg)
        })?
        .build();

    let mut pk_record_key = vec![];
    pk_record_key.put_slice("/pk/".as_bytes());
    pk_record_key.put_slice(swarm.local_peer_id().to_bytes().as_slice());

    let mut pk_record = kad::Record::new(pk_record_key, "\0".as_bytes().to_vec());
    pk_record.publisher = Some(*swarm.local_peer_id());
    pk_record.expires = Some(Instant::now() + Duration::from_secs(60));

    swarm
        .behaviour_mut()
        .put_record(pk_record, kad::Quorum::N(NonZeroUsize::new(3).unwrap()))?;

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
            transport_id: local_key.public().to_peer_id(),
        })
        .await;

    info!("Waiting for result from command Command::FindMultiaddr");

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

    let _ = user_handles[USERS_NUM - 1]
        .command
        .send(Command::FindMultiaddr {
            transport_id: local_key.public().to_peer_id(),
        })
        .await;

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

    const IPFS_PROTO_NAME: StreamProtocol = StreamProtocol::new("/ipfs/kad/1.0.0");

    info!(users = USERS_NUM, "Setting up users in all-to-all topology");

    let Setup {
        mut user_handles,
        cancel,
        tasks,
    } = Setup::all_to_all(USERS_NUM).await?;

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
            // Create a Kademlia behaviour.
            let mut cfg = kad::Config::new(IPFS_PROTO_NAME);
            cfg.set_query_timeout(Duration::from_secs(5 * 60));
            let store = kad::store::MemoryStore::new(key.public().to_peer_id());
            kad::Behaviour::with_config(key.public().to_peer_id(), store, cfg)
        })?
        .build();

    let another_keypair = identity::Keypair::generate_ed25519();

    let mut pk_record_key = vec![];
    pk_record_key.put_slice(another_keypair.public().to_peer_id().to_bytes().as_slice());

    let mut pk_record = kad::Record::new(
        pk_record_key,
        flexbuffers::to_vec(
            SignedRecord::new(
                RecordData::new(
                    local_key.public(),
                    swarm.listeners().cloned().collect::<Vec<_>>(),
                ),
                &TransportKeypairSigner::new(local_key),
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
        .put_record(pk_record, kad::Quorum::N(NonZeroUsize::new(3).unwrap()))?;

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
            transport_id: another_keypair.public().to_peer_id(),
        })
        .await;

    info!("Waiting for result from command Command::FindMultiaddr");

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
