//! Test QUIC and TCP connectivity on IPv4 and IPv6.

use std::{sync::Arc, time::Duration};

use libp2p::{Multiaddr, identity::Keypair};
use tokio::{join, spawn, sync::oneshot, time};
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::{
    commands::{Command, QueryP2PStateCommand},
    tests::common::{MockApplicationSigner, User, init_tracing},
    validator::DefaultP2PValidator,
};

/// Test QUIC and TCP connectivity on IPv4 and IPv6.
#[tokio::test]
async fn test_quic_and_tcp_connectivity_ipv4_ipv6() {
    init_tracing();
    let tcp4_base: Multiaddr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
    let quic4_base: Multiaddr = "/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap();
    let tcp6_base: Multiaddr = "/ip6/::1/tcp/0".parse().unwrap();
    let quic6_base: Multiaddr = "/ip6/::1/udp/0/quic-v1".parse().unwrap();

    let keypair_a = Keypair::generate_ed25519();
    let keypair_b = Keypair::generate_ed25519();

    let cancel = CancellationToken::new();

    let user_a = User::new(
        keypair_a.clone(),
        keypair_a.clone(),
        vec![],
        vec![keypair_b.public()],
        vec![
            tcp4_base.clone(),
            quic4_base.clone(),
            tcp6_base.clone(),
            quic6_base.clone(),
        ],
        cancel.clone(),
        Arc::new(MockApplicationSigner::new(keypair_a.clone())),
        Box::new(DefaultP2PValidator),
    )
    .expect("Failed to create listening node A");

    let user_b = User::new(
        keypair_b.clone(),
        keypair_b.clone(),
        vec![],
        vec![keypair_a.public()],
        vec![],
        cancel.clone(),
        Arc::new(MockApplicationSigner::new(keypair_b.clone())),
        Box::new(DefaultP2PValidator),
    )
    .expect("Failed to create connecting node B");

    let command_a = user_a.command.clone();
    let command_b = user_b.command.clone();

    // Spawn P2P listen tasks
    let task_a = spawn(async move { user_a.p2p.listen().await });
    let task_b = spawn(async move { user_b.p2p.listen().await });

    time::sleep(Duration::from_millis(1_000)).await;

    let (tx, rx) = oneshot::channel();
    let query = QueryP2PStateCommand::GetMyListeningAddresses {
        response_sender: tx,
    };
    command_a.send_command(Command::QueryP2PState(query)).await;
    let listening_addresses = rx.await.expect("Failed to get listening addresses");

    info!("Node A listening addresses: {:?}", listening_addresses);
    assert!(
        listening_addresses.len() >= 4,
        "Should have at least 4 protocol listeners"
    );

    for addr in listening_addresses {
        info!(%addr, "Testing connection");

        let connect_cmd = Command::ConnectToPeer {
            app_public_key: keypair_a.public(),
            addresses: vec![addr.clone()],
        };
        command_b.send_command(connect_cmd).await;

        time::sleep(Duration::from_millis(200)).await;

        let (tx, rx) = oneshot::channel();
        let is_connected_query = QueryP2PStateCommand::IsConnected {
            app_public_key: keypair_a.public(),
            response_sender: tx,
        };
        command_b
            .send_command(Command::QueryP2PState(is_connected_query))
            .await;
        let is_connected = rx.await.expect("Failed to check connection status");

        assert!(is_connected, "Failed to establish {addr} connection");

        command_b
            .send_command(Command::DisconnectFromPeer {
                target_app_public_key: keypair_a.public(),
            })
            .await;
        info!(%addr, "Disconnect requested");

        time::sleep(Duration::from_millis(200)).await;
        let (tx, rx) = oneshot::channel();
        let is_connected_query = QueryP2PStateCommand::IsConnected {
            app_public_key: keypair_a.public(),
            response_sender: tx,
        };
        command_b
            .send_command(Command::QueryP2PState(is_connected_query))
            .await;
        let is_connected = rx
            .await
            .expect("Failed to check connection status after disconnect");
        assert!(
            !is_connected,
            "{addr} connection was not properly disconnected"
        );
    }

    info!("All QUIC and TCP connections (IPv4 and IPv6) succeeded!");
    cancel.cancel();
    let _ = join!(task_a, task_b);
}

/// Test TCP fallback when QUIC connection fails.
#[tokio::test]
async fn test_tcp_fallback_on_quic_failure() {
    init_tracing();

    let tcp4_addr: Multiaddr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
    let quic4_addr: Multiaddr = "/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap();

    let keypair_a = Keypair::generate_ed25519();
    let keypair_b = Keypair::generate_ed25519();
    let cancel = CancellationToken::new();

    let user_a = User::new(
        keypair_a.clone(),
        keypair_a.clone(),
        vec![],
        vec![keypair_b.public()],
        vec![tcp4_addr.clone(), quic4_addr.clone()],
        cancel.clone(),
        Arc::new(MockApplicationSigner::new(keypair_a.clone())),
        Box::new(DefaultP2PValidator),
    )
    .expect("Failed to create listening node");

    let user_b = User::new(
        keypair_b.clone(),
        keypair_b.clone(),
        vec![],
        vec![keypair_a.public()],
        vec![],
        cancel.clone(),
        Arc::new(MockApplicationSigner::new(keypair_b.clone())),
        Box::new(DefaultP2PValidator),
    )
    .expect("Failed to create connecting node");

    let command_a = user_a.command.clone();
    let command_b = user_b.command.clone();

    let task_a = spawn(async move { user_a.p2p.listen().await });
    let task_b = spawn(async move { user_b.p2p.listen().await });

    time::sleep(Duration::from_millis(1_000)).await;

    let (tx, rx) = oneshot::channel();
    let query = QueryP2PStateCommand::GetMyListeningAddresses {
        response_sender: tx,
    };
    command_a.send_command(Command::QueryP2PState(query)).await;
    let listening_addresses = rx.await.expect("Failed to get listening addresses");

    info!("Node A listening addresses: {:?}", listening_addresses);

    let actual_tcp_addr = listening_addresses
        .iter()
        .find(|addr| addr.protocol_stack().any(|proto| proto.contains("tcp")))
        .expect("Should have a TCP address")
        .clone();

    let invalid_quic_addr: Multiaddr = "/ip4/192.0.2.1/udp/12345/quic-v1".parse().unwrap();

    let mixed_addresses = vec![invalid_quic_addr.clone(), actual_tcp_addr.clone()];

    let connect_cmd = Command::ConnectToPeer {
        app_public_key: keypair_a.public(),
        addresses: mixed_addresses,
    };
    command_b.send_command(connect_cmd).await;

    time::sleep(Duration::from_secs(10)).await;

    let (tx, rx) = oneshot::channel();
    let is_connected_query = QueryP2PStateCommand::IsConnected {
        app_public_key: keypair_a.public(),
        response_sender: tx,
    };
    command_b
        .send_command(Command::QueryP2PState(is_connected_query))
        .await;
    let is_connected = rx.await.expect("Failed to check connection status");

    assert!(
        is_connected,
        "Failed to establish connection via TCP fallback after QUIC failure"
    );

    info!("TCP fallback succeeded after QUIC connection failed");

    cancel.cancel();
    let _ = join!(task_a, task_b);
}
