#[cfg(feature = "byos")]
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::bail;
use futures::StreamExt;
use libp2p::{
    Multiaddr, StreamProtocol, Transport, build_multiaddr,
    bytes::BufMut,
    core::{muxing::StreamMuxerBox, transport::MemoryTransport},
    identify::{Behaviour as Identify, Config},
    identity,
    kad::{self, Event as KadEvent, Quorum, RecordKey, store::RecordStore},
    noise,
    swarm::{NetworkBehaviour, SwarmEvent, dial_opts::DialOpts},
    yamux,
};
use tokio::sync::oneshot::channel;
use tracing::info;

use super::common::Setup;
#[cfg(not(feature = "byos"))]
use crate::signer::TransportKeypairSigner;
#[cfg(feature = "request-response")]
use crate::swarm::behavior::RequestResponseRawBehaviour;
#[cfg(feature = "byos")]
use crate::tests::common::MockApplicationSigner;
use crate::{
    commands::{Command, QueryP2PStateCommand},
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

    /// Gossipsub
    #[cfg(feature = "gossipsub")]
    pub gossipsub: libp2p::gossipsub::Behaviour,

    /// RequestResponse
    #[cfg(feature = "request-response")]
    pub req_resp: RequestResponseRawBehaviour,

    /// Setup
    #[cfg(feature = "byos")]
    pub setup: crate::swarm::setup::behavior::SetupBehaviour,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn test_kad_replay_attack_protection() -> anyhow::Result<()> {
    init_tracing();

    let alice_key = identity::Keypair::generate_ed25519();
    let alice_peer_id = alice_key.public().to_peer_id();

    // 1. Setup Bob (Victim) using the standard helper
    #[cfg(feature = "byos")]
    let setup_res = Setup::all_to_all_with_new_user_allowlist(1, &alice_key.public()).await?;
    #[cfg(not(feature = "byos"))]
    let setup_res = Setup::all_to_all(1).await?;

    let Setup {
        user_handles,
        cancel,
        tasks,
    } = setup_res;

    let bob_handle = &user_handles[0];
    let bob_peer_id = bob_handle.peer_id;

    // Get Bob's address
    let (tx, rx) = channel::<Vec<Multiaddr>>();
    bob_handle
        .command
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetMyListeningAddresses {
                response_sender: tx,
            },
        ))
        .await;
    let bob_addrs = rx.await.unwrap();
    let bob_addr = bob_addrs[0].clone();
    info!(%bob_addr, "Bob's address");

    // 2. Setup Alice (Attacker) manually

    let mut alice_swarm = libp2p::SwarmBuilder::with_existing_identity(alice_key.clone())
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

            #[cfg(feature = "gossipsub")]
            let gossipsub = libp2p::gossipsub::Behaviour::new(
                libp2p::gossipsub::MessageAuthenticity::Signed(key.clone()),
                libp2p::gossipsub::Config::default(),
            )
            .unwrap();

            #[cfg(feature = "request-response")]
            let req_resp = RequestResponseRawBehaviour::new(
                [(
                    StreamProtocol::new(PROTOCOL_NAME),
                    libp2p::request_response::ProtocolSupport::Full,
                )],
                libp2p::request_response::Config::default(),
            );

            #[cfg(feature = "byos")]
            let setup = crate::swarm::setup::behavior::SetupBehaviour::new(
                key.public(),
                key.public().to_peer_id(),
                Arc::new(MockApplicationSigner::new(key.clone())),
                Duration::from_secs(300),
                Duration::from_secs(5),
            );

            Behaviour {
                kademlia: kad,
                identify: Identify::new(Config::new(PROTOCOL_NAME.to_string(), key.public())),
                #[cfg(feature = "gossipsub")]
                gossipsub,
                #[cfg(feature = "request-response")]
                req_resp,
                #[cfg(feature = "byos")]
                setup,
            }
        })?
        .build();

    alice_swarm.listen_on(build_multiaddr!(Memory(0u64)))?; // 0 means random port

    // 3. Connect Alice to Bob
    alice_swarm.dial(
        DialOpts::peer_id(bob_peer_id)
            .addresses(vec![bob_addr.clone()])
            .condition(libp2p::swarm::dial_opts::PeerCondition::Always)
            .build(),
    )?;

    // Run Alice loop
    let mut alice_flag = 0;
    let mut r1_record: Option<kad::Record> = None;
    let mut r2_record: Option<kad::Record> = None;

    // Key for the record
    let mut pk_record_key = vec![];
    pk_record_key.put_slice(alice_peer_id.to_bytes().as_slice());
    let record_key = RecordKey::new(&pk_record_key);

    loop {
        tokio::select! {
            event = alice_swarm.select_next_some() => {
                match event {
                    SwarmEvent::ConnectionEstablished { peer_id, .. } if peer_id == bob_peer_id => {
                        info!("Alice connected to Bob");

                        // Add Bob to Kademlia routing table
                        alice_swarm.behaviour_mut().kademlia.add_address(&peer_id, bob_addr.clone());

                        // Create R1
                        #[cfg(feature = "byos")]
                        let signer = MockApplicationSigner::new(alice_key.clone());
                        #[cfg(not(feature = "byos"))]
                        let signer = TransportKeypairSigner::new(alice_key.clone());

                        #[cfg(feature = "byos")]
                        let signer_ref = &signer;
                        #[cfg(not(feature = "byos"))]
                        let signer_ref = &signer;

                        let r1_signed = SignedRecord::new(
                            RecordData::new(
                                #[cfg(feature = "byos")]
                                alice_key.public(),
                                #[cfg(not(feature = "byos"))]
                                alice_key.public(),
                                vec![],
                            ),
                            signer_ref,
                        ).await.unwrap();

                        let r1_bytes = flexbuffers::to_vec(r1_signed).unwrap();

                        let mut rec = kad::Record::new(record_key.clone(), r1_bytes);
                        rec.publisher = Some(alice_peer_id);
                        rec.expires = Some(Instant::now() + Duration::from_secs(60));
                        r1_record = Some(rec.clone());

                        info!("Alice putting R1");
                        alice_swarm.behaviour_mut().kademlia.put_record(rec, Quorum::One).unwrap();
                        alice_flag = 1;
                    }
                    SwarmEvent::Behaviour(BehaviourEvent::Kademlia(KadEvent::OutboundQueryProgressed { result: kad::QueryResult::PutRecord(Ok(_)), .. })) => {
                         info!("PutRecord succeeded");
                         if alice_flag == 1 {
                             // R1 put done. Wait a bit then put R2.
                             tokio::time::sleep(Duration::from_secs(2)).await;

                             #[cfg(feature = "byos")]
                             let signer = MockApplicationSigner::new(alice_key.clone());
                             #[cfg(not(feature = "byos"))]
                             let signer = TransportKeypairSigner::new(alice_key.clone());

                             #[cfg(feature = "byos")]
                             let signer_ref = &signer;
                             #[cfg(not(feature = "byos"))]
                             let signer_ref = &signer;

                             // Create R2 (newer timestamp because time passed)
                             let r2_signed = SignedRecord::new(
                                RecordData::new(
                                    #[cfg(feature = "byos")]
                                    alice_key.public(),
                                    #[cfg(not(feature = "byos"))]
                                    alice_key.public(),
                                    vec![build_multiaddr!(Memory(1234u64))], // Different data
                                ),
                                signer_ref,
                            ).await.unwrap();

                            let r2_bytes = flexbuffers::to_vec(r2_signed).unwrap();
                            let mut rec = kad::Record::new(record_key.clone(), r2_bytes);
                            rec.publisher = Some(alice_peer_id);
                            rec.expires = Some(Instant::now() + Duration::from_secs(60));
                            r2_record = Some(rec.clone());

                            info!("Alice putting R2");
                            alice_swarm.behaviour_mut().kademlia.put_record(rec, Quorum::One).unwrap();
                            alice_flag = 2;
                         } else if alice_flag == 2 {
                             // R2 put done.
                             // Now replay R1.
                             info!("Alice REPLAYING R1");
                             let rec = r1_record.clone().unwrap();
                             alice_swarm.behaviour_mut().kademlia.put_record(rec, Quorum::One).unwrap();
                             alice_flag = 3;
                         } else if alice_flag == 3 {
                             // Replay done.
                             // Now verify what Bob has.

                             // Clear local store to ensure we fetch from Bob
                             alice_swarm.behaviour_mut().kademlia.store_mut().remove(&record_key);

                             info!("Alice asking Bob for record");
                             alice_swarm.behaviour_mut().kademlia.get_record(record_key.clone());
                             alice_flag = 4;
                         }
                    }
                    SwarmEvent::Behaviour(BehaviourEvent::Kademlia(KadEvent::OutboundQueryProgressed { result: kad::QueryResult::GetRecord(Ok(kad::GetRecordOk::FoundRecord(peer_record))), .. })) => {
                        info!("Got record from Bob");
                        let bytes = peer_record.record.value;
                        let signed_record: SignedRecord = flexbuffers::from_slice(&bytes).unwrap();

                        // Check if it matches R2
                        let r2_bytes = r2_record.clone().unwrap().value;
                        let r2_signed: SignedRecord = flexbuffers::from_slice(&r2_bytes).unwrap();

                        if signed_record.message.date == r2_signed.message.date {
                            info!("Success: Bob has R2");
                            break;
                        } else {
                             let r1_bytes = r1_record.clone().unwrap().value;
                             let r1_signed: SignedRecord = flexbuffers::from_slice(&r1_bytes).unwrap();

                             if signed_record.message.date == r1_signed.message.date {
                                 bail!("Failure: Bob accepted the replay attack (has R1)");
                             } else {
                                 bail!("Failure: Bob has unknown record");
                             }
                        }
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                bail!("Test timed out");
            }
        }
    }

    cancel.cancel();
    tasks.wait().await;
    Ok(())
}
