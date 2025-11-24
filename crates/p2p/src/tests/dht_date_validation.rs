use std::{
    error,
    future::Future,
    pin::Pin,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use libp2p::{
    StreamProtocol, Transport,
    core::{muxing::StreamMuxerBox, transport::MemoryTransport},
    identify::{Behaviour as Identify, Config},
    identity,
    kad::{self, Record, RecordKey, store::RecordStore},
    noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    yamux,
};
use tokio::{
    select,
    sync::oneshot,
    time::{sleep, timeout},
};
use tokio_stream::StreamExt;
use tracing::info;

use super::common::Setup;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    events::CommandEvent,
    signer::ApplicationSigner,
    swarm::{
        DEFAULT_ENVELOPE_MAX_AGE,
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

#[derive(Debug, Clone)]
struct TestSigner {
    keypair: identity::Keypair,
}

impl TestSigner {
    fn new(keypair: identity::Keypair) -> Self {
        Self { keypair }
    }
}

impl ApplicationSigner for TestSigner {
    fn sign<'life0, 'life1, 'async_trait>(
        &'life0 self,
        message: &'life1 [u8],
    ) -> Pin<
        Box<
            dyn Future<Output = Result<[u8; 64], Box<dyn error::Error + Send + Sync>>>
                + Send
                + Sync
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let signature = self.keypair.sign(message)?;
            let mut array = [0u8; 64];
            if signature.len() != 64 {
                return Err("Signature length is not 64 bytes".into());
            }
            array.copy_from_slice(&signature);
            Ok(array)
        })
    }
}

// Helper to create a signed record with a specific date
async fn create_signed_record(
    keypair: &identity::Keypair,
    date: u64,
) -> anyhow::Result<SignedRecord> {
    let signer = TestSigner::new(keypair.clone());
    let mut record_data = RecordData::new(keypair.public(), vec![]);
    record_data.date = date;

    SignedRecord::new(record_data, &signer)
        .await
        .map_err(|e| anyhow::anyhow!(e))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_dht_date_validation() -> anyhow::Result<()> {
    init_tracing();

    // 1. Setup a user (receiver)
    let Setup {
        mut user_handles,
        cancel: _cancel,
        tasks: _tasks,
    } = Setup::all_to_all(1).await?;

    let receiver_handle = user_handles.pop().unwrap();
    let receiver_peer_id = receiver_handle.peer_id;
    let mut event_rx = receiver_handle.command.get_new_receiver();

    // Get receiver's address
    let (tx, rx) = oneshot::channel();
    receiver_handle
        .command
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetMyListeningAddresses {
                response_sender: tx,
            },
        ))
        .await;
    let receiver_addrs = rx.await.unwrap();
    let receiver_addr = receiver_addrs[0].clone();

    // 2. Setup a raw attacker swarm
    let attacker_key = identity::Keypair::generate_ed25519();
    let attacker_peer_id = attacker_key.public().to_peer_id();

    let mut attacker_swarm = libp2p::SwarmBuilder::with_existing_identity(attacker_key.clone())
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
            Behaviour {
                identify: Identify::new(Config::new("/strata/0.0.1".to_string(), key.public())),
                kademlia: kad::Behaviour::with_config(key.public().to_peer_id(), store, cfg),
            }
        })?
        .build();

    // Connect attacker to receiver
    attacker_swarm.dial(receiver_addr)?;

    // Wait for connection
    let mut connected = false;
    while !connected {
        if let Some(SwarmEvent::ConnectionEstablished { peer_id, .. }) = attacker_swarm.next().await
            && peer_id == receiver_peer_id
        {
            connected = true;
            info!("Attacker connected to receiver");
        }
    }

    // 3. Prepare invalid records
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let max_age = DEFAULT_ENVELOPE_MAX_AGE.as_secs();

    // Use a separate key for records to query properly
    let record_key_pair = identity::Keypair::generate_ed25519();

    // Case 1: u64::MAX
    let record_max = create_signed_record(&record_key_pair, u64::MAX).await?;

    // Case 2: Future date (now + 1 hour)
    let record_future = create_signed_record(&record_key_pair, now + 3600).await?;

    // Case 3: Old date (now - max_age - 1 hour)
    let record_old = create_signed_record(&record_key_pair, now - max_age - 3600).await?;

    for (record, name) in [
        (record_max, "MAX_DATE"),
        (record_future, "FUTURE_DATE"),
        (record_old, "OLD_DATE"),
    ] {
        info!("Testing {}", name);
        let record_bytes = flexbuffers::to_vec(&record).unwrap();

        #[cfg(feature = "byos")]
        let key_bytes = record_key_pair.public().encode_protobuf();
        #[cfg(not(feature = "byos"))]
        let key_bytes = record_key_pair.public().to_peer_id().to_bytes();

        let key = RecordKey::new(&key_bytes);
        let kad_record = Record {
            key: key.clone(),
            value: record_bytes,
            publisher: Some(attacker_peer_id),
            expires: None,
        };

        // Attacker puts the record locally and replicates it (or just holds it for query)
        // Since we want to test Get path, ensuring attacker holds it is enough if we query
        // attacker. But we want to verify Receiver behavior.
        // If we use put_record, it replicates to Receiver. Receiver should REJECT it.
        // Then if we query Receiver, it should NOT find it locally.
        // But if Receiver asks Attacker (closest peer), Attacker returns it.
        // Then Receiver validates response and SHOULD REJECT it.

        // So let's put it in Attacker's store.
        attacker_swarm
            .behaviour_mut()
            .kademlia
            .store_mut()
            .put(kad_record.clone())
            .expect("Failed to put record");

        // Also trigger put_record to try to push it to receiver (testing "put" rejection)
        attacker_swarm
            .behaviour_mut()
            .kademlia
            .put_record(kad_record, kad::Quorum::One)
            .expect("Failed to replicate record");

        // Run attacker loop briefly to handle outgoing messages
        let _ = timeout(Duration::from_millis(200), async {
            loop {
                attacker_swarm.next().await;
            }
        })
        .await;

        // Now query from Receiver
        #[cfg(feature = "byos")]
        let cmd = Command::FindMultiaddr {
            app_public_key: record_key_pair.public(),
        };
        #[cfg(not(feature = "byos"))]
        let cmd = Command::FindMultiaddr {
            transport_id: record_key_pair.public().to_peer_id(),
        };

        receiver_handle.command.send_command(cmd).await;

        // Wait for ResultFindMultiaddress
        // We also need to keep driving the attacker swarm because Receiver might query it
        loop {
            select! {
                        res = event_rx.recv() => {
                            match res {
                                Ok(CommandEvent::ResultFindMultiaddress(opt_addrs)) => {
                                    if opt_addrs.is_some() {
                                        anyhow::bail!("Receiver accepted invalid record {}", name);
                                    } else {
                                        info!("Receiver correctly rejected {}", name);
                                    }
                                    break;
                                }
                                Err(e) => {
                                     anyhow::bail!("Event channel error: {}", e);
                                }
                            }
                        }
                        _ = attacker_swarm.next() => {
                            // Drive attacker
                        }
                        _ = sleep(Duration::from_secs(5)) => {
                            anyhow::bail!("Timeout waiting for ResultFindMultiaddress");
                }
            }
        }
    }

    Ok(())
}
