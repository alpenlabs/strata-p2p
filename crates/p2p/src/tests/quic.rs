//! Test QUIC and TCP connectivity on IPv4 and IPv6.

use std::time::Duration;

use libp2p::{Multiaddr, identity::Keypair};
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::{
    commands::{Command, QueryP2PStateCommand},
    tests::common::{MockApplicationSigner, User},
};

#[tokio::test]
async fn test_quic_and_tcp_connectivity_ipv4_ipv6() {
    let tcp4_base: Multiaddr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();
    let quic4_base: Multiaddr = "/ip4/127.0.0.1/udp/0/quic-v1".parse().unwrap();
    let tcp6_base: Multiaddr = "/ip6/::1/tcp/0".parse().unwrap();
    let quic6_base: Multiaddr = "/ip6/::1/udp/0/quic-v1".parse().unwrap();

    let keypair_a = Keypair::generate_ed25519();
    let keypair_b = Keypair::generate_ed25519();
    let peer_id_a = keypair_a.public().to_peer_id();
    let peer_id_b = keypair_b.public().to_peer_id();

    let cancel = CancellationToken::new();

    let user_a = User::new(
        keypair_a.clone(),        // app_keypair
        keypair_a.clone(),        // transport_keypair
        vec![],                   // connect_to
        vec![keypair_b.public()], // allowlist
        vec![
            tcp4_base.clone(),
            quic4_base.clone(),
            tcp6_base.clone(),
            quic6_base.clone(),
        ], // listening_addrs
        cancel.clone(),
        MockApplicationSigner::new(keypair_a.clone()),
    )
    .expect("Failed to create listening node A");

    let user_b = User::new(
        keypair_b.clone(),        // app_keypair
        keypair_b.clone(),        // transport_keypair
        vec![],                   // connect_to
        vec![keypair_a.public()], // allowlist
        vec![],                   // listening_addrs
        cancel.clone(),
        MockApplicationSigner::new(keypair_b.clone()),
    )
    .expect("Failed to create connecting node B");

    let command_a = user_a.command.clone();
    let command_b = user_b.command.clone();

    // Spawn P2P listen tasks
    let task_a = tokio::spawn(async move { user_a.p2p.listen().await });
    let task_b = tokio::spawn(async move { user_b.p2p.listen().await });

    tokio::time::sleep(Duration::from_millis(1000)).await;

    let (tx, rx) = tokio::sync::oneshot::channel();
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

    let (tx, rx) = tokio::sync::oneshot::channel();
    command_a
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetMyListeningAddresses {
                response_sender: tx,
            },
        ))
        .await;
    let listening_addresses = rx.await.expect("Failed to get listening addresses");
    info!(?listening_addresses, "Node A listening addresses");
    assert!(
        listening_addresses.len() == 4,
        "Should have 4 listening addresses"
    );

    for addr in listening_addresses {
        info!(%addr, "Testing connection");

        let connect_cmd = Command::ConnectToPeer {
            app_public_key: keypair_a.public(),
            addresses: vec![addr.clone()],
        };
        command_b.send_command(connect_cmd).await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        let (tx, rx) = tokio::sync::oneshot::channel();
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
            .send_command(Command::DisconnectFromPeer { peer_id: peer_id_a })
            .await;
        info!(%addr, "Disconnect requested");

        tokio::time::sleep(Duration::from_millis(200)).await;
        let (tx, rx) = tokio::sync::oneshot::channel();
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
    let _ = tokio::join!(task_a, task_b);
}
