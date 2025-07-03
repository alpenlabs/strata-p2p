//! Three-handlers integration test for new handler-based architecture.

use std::time::Duration;

use libp2p::{Multiaddr, PeerId, build_multiaddr, identity::Keypair};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::{
    commands::Command,
    events::{GossipEvent, ReqRespEvent},
    swarm::{
        P2P, P2PConfig,
        handle::{CommandHandle, GossipHandle, ReqRespHandle},
    },
};

struct UserWithHandlers {
    p2p: Option<P2P>,
    gossip: GossipHandle,
    command: CommandHandle,
    reqresp: Option<ReqRespHandle>,
    peer_id: PeerId,
    kp: Keypair,
}

impl UserWithHandlers {
    fn new(
        keypair: Keypair,
        allowlist: Vec<PeerId>,
        connect_to: Vec<Multiaddr>,
        local_addr: Multiaddr,
        cancel: CancellationToken,
    ) -> anyhow::Result<Self> {
        let config = P2PConfig {
            keypair: keypair.clone(),
            idle_connection_timeout: Duration::from_secs(30),
            max_retries: None,
            dial_timeout: None,
            general_timeout: None,
            connection_check_interval: None,
            listening_addr: local_addr,
            allowlist,
            connect_to,
        };
        let swarm = crate::swarm::with_inmemory_transport(&config)?;
        let (p2p, reqresp) = P2P::from_config(config, cancel, swarm, None)?;
        let gossip = p2p.new_gossip_handle();
        let command = p2p.new_command_handle();
        let peer_id = p2p.local_peer_id();
        Ok(Self {
            p2p: Some(p2p),
            gossip,
            command,
            reqresp: Some(reqresp),
            peer_id,
            kp: keypair,
        })
    }
}

/// Helper for all-to-all setup with UserWithHandlers
struct SetupWithHandlers {
    cancel: CancellationToken,
    users: Vec<UserWithHandlers>,
    join_handles: Vec<tokio::task::JoinHandle<()>>,
}

impl SetupWithHandlers {
    async fn all_to_all(n: usize) -> anyhow::Result<Self> {
        let keypairs: Vec<_> = (0..n).map(|_| Keypair::generate_ed25519()).collect();
        let peer_ids: Vec<_> = keypairs
            .iter()
            .map(|k| PeerId::from_public_key(&k.public()))
            .collect();
        let base_addr = 100_000_000u64;
        let addrs: Vec<_> = (0..n)
            .map(|i| build_multiaddr!(Memory(base_addr + i as u64)))
            .collect();
        let cancel = CancellationToken::new();
        let mut users = Vec::new();
        for i in 0..n {
            let mut allowlist = peer_ids.clone();
            allowlist.remove(i);
            let mut connect_to = addrs.clone();
            connect_to.remove(i);
            println!(
                "[DEBUG] Node {} allowlist: {:?}, connect_to: {:?}",
                i + 1,
                allowlist,
                connect_to
            );
            let user = UserWithHandlers::new(
                keypairs[i].clone(),
                allowlist,
                connect_to,
                addrs[i].clone(),
                cancel.child_token(),
            )?;
            users.push(user);
        }

        for user in &mut users {
            let p2p = user.p2p.as_mut().expect("P2P should be present");
            p2p.establish_connections().await;
        }
        // Spawn listen tasks
        let mut join_handles = Vec::new();
        for user in &mut users {
            let p2p = user.p2p.take().expect("P2P already taken");
            let handle = tokio::spawn(async move { p2p.listen().await });
            join_handles.push(handle);
        }
        Ok(Self {
            cancel,
            users,
            join_handles,
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn three_handlers_integration() -> anyhow::Result<()> {
    // --- 1. Setup three nodes with their own handlers ---
    let SetupWithHandlers {
        cancel,
        mut users,
        join_handles,
    } = SetupWithHandlers::all_to_all(3).await?;

    let mut all_connected = false;
    for attempt in 0..10 {
        all_connected = true;
        for (i, user) in users.iter().enumerate() {
            let connected = user.command.get_connected_peers().await;
            println!("[DEBUG] Node {} connected peers: {:?}", i + 1, connected);
            if connected.len() < 2 {
                all_connected = false;
            }
        }
        if all_connected {
            println!(
                "[TEST] All nodes are fully connected after {} attempts",
                attempt + 1
            );
            break;
        }
    }
    if !all_connected {
        anyhow::bail!("Not all nodes are fully connected after waiting");
    }

    // --- 2. Node 1 publishes a gossipsub message ---
    let msg = b"hello from node1".to_vec();
    users[0]
        .command
        .send_command(Command::PublishMessage { data: msg.clone() })
        .await;
    println!("[TEST] Node 1 published gossipsub message");

    // --- 3. All nodes should receive the message via their GossipHandle ---
    for i in 0..3 {
        if i == 0 {
            continue;
        }
        println!(
            "[TEST] Waiting for node {} to receive gossip event...",
            i + 1
        );
        match users[i].gossip.next_event().await {
            Ok(event) => match event {
                GossipEvent::ReceivedMessage(data) => {
                    println!("[TEST] Node {} received gossip message: {:?}", i + 1, data);
                    assert_eq!(
                        data,
                        msg,
                        "Node {} did not receive the correct gossip message",
                        i + 1
                    );
                }
            },
            Err(e) => {
                println!(
                    "[TEST] Node {} did not receive a gossip event: {:?}",
                    i + 1,
                    e
                );
                anyhow::bail!("Node {} did not receive a gossip event", i + 1);
            }
        }
    }
    println!("[TEST] All nodes received gossip message");

    let req_msg = b"request from node2".to_vec();
    println!("[TEST] Node 2 sending request to Node 3");
    users[1]
        .command
        .send_command(Command::RequestMessage {
            peer_id: users[2].peer_id,
            data: req_msg.clone(),
        })
        .await;
    println!("[TEST] Node 2 sent request to Node 3");

    println!("[TEST] Waiting for Node 3 to receive request event...");
    let event = timeout(
        Duration::from_secs(5),
        users[2].reqresp.as_mut().unwrap().next_event(),
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("Node 3 did not receive a request event"))?;
    match event {
        ReqRespEvent::CustomEvent(data, responder) => {
            println!("[TEST] Node 3 received request: {:?}", data);
            assert_eq!(data, req_msg, "Node 3 did not receive the correct request");
            let _ = responder.send(b"response from node3".to_vec());
            println!("[TEST] Node 3 sent response");
        }
        _ => anyhow::bail!("Node 3 did not receive a CustomEvent request"),
    }

    let event = timeout(
        Duration::from_secs(5),
        users[1].reqresp.as_mut().unwrap().next_event(),
    )
    .await?
    .ok_or_else(|| anyhow::anyhow!("Node 2 did not receive a response event"))?;
    match event {
        ReqRespEvent::ReceivedMessage(resp) => {
            println!("[TEST] Node 2 received response: {:?}", resp);
            assert_eq!(
                resp,
                b"response from node3".to_vec(),
                "Node 2 did not receive the correct response"
            );
        }
        _ => anyhow::bail!("Node 2 did not receive a response message"),
    }

    cancel.cancel();
    for handle in join_handles {
        let _ = handle.await;
    }

    Ok(())
}
