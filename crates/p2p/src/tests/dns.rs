//! Test DNS resolution for peer connections.

#[cfg(feature = "byos")]
use std::sync::Arc;
use std::time::Duration;

use libp2p::{Multiaddr, connection_limits::ConnectionLimits, identity::Keypair};
use tokio::{join, spawn, sync::oneshot, time};
use tokio_util::sync::CancellationToken;
use tracing::info;

#[cfg(feature = "byos")]
use crate::tests::common::MockApplicationSigner;
#[cfg(feature = "mem-conn-limits-abs")]
use crate::tests::common::SIXTEEN_GIBIBYTES;
#[cfg(all(
    any(feature = "gossipsub", feature = "request-response"),
    not(feature = "byos")
))]
use crate::validator::DefaultP2PValidator;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    tests::common::{User, init_tracing},
};

/// Test that a peer can connect to another peer using a `/dns4/localhost/...` address.
///
/// Node A listens on a TCP address with a random port. Node B then connects
/// using a `dns4/localhost` address with the same port, verifying that the DNS
/// transport layer resolves `localhost` to `127.0.0.1`.
#[tokio::test]
async fn test_dns4_localhost_resolution() {
    init_tracing();

    let tcp_addr: Multiaddr = "/ip4/127.0.0.1/tcp/0".parse().unwrap();

    let keypair_a = Keypair::generate_ed25519();
    let keypair_b = Keypair::generate_ed25519();

    let cancel = CancellationToken::new();

    // Node A: listener
    let user_a = User::new(
        #[cfg(feature = "byos")]
        keypair_a.clone(),
        keypair_a.clone(),
        vec![],
        #[cfg(feature = "byos")]
        vec![keypair_b.public()],
        vec![tcp_addr],
        cancel.clone(),
        #[cfg(feature = "byos")]
        Arc::new(MockApplicationSigner::new(keypair_a.clone())),
        #[cfg(all(
            any(feature = "gossipsub", feature = "request-response"),
            not(feature = "byos")
        ))]
        Box::new(DefaultP2PValidator),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
    )
    .expect("Failed to create listening node A");

    // Node B: dialer
    let user_b = User::new(
        #[cfg(feature = "byos")]
        keypair_b.clone(),
        keypair_b.clone(),
        vec![],
        #[cfg(feature = "byos")]
        vec![keypair_a.public()],
        vec![],
        cancel.clone(),
        #[cfg(feature = "byos")]
        Arc::new(MockApplicationSigner::new(keypair_b.clone())),
        #[cfg(all(
            any(feature = "gossipsub", feature = "request-response"),
            not(feature = "byos")
        ))]
        Box::new(DefaultP2PValidator),
        ConnectionLimits::default().with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        1.0,
        #[cfg(feature = "kad")]
        None,
        #[cfg(feature = "kad")]
        None,
    )
    .expect("Failed to create connecting node B");

    let command_a = user_a.command.clone();
    let command_b = user_b.command.clone();

    let task_a = spawn(async move { user_a.p2p.listen().await });
    let task_b = spawn(async move { user_b.p2p.listen().await });

    time::sleep(Duration::from_millis(1_000)).await;

    // Get node A's actual listening address to extract the assigned port
    let (tx, rx) = oneshot::channel();
    command_a
        .send_command(Command::QueryP2PState(
            QueryP2PStateCommand::GetMyListeningAddresses {
                response_sender: tx,
            },
        ))
        .await;
    let listening_addresses = rx.await.expect("Failed to get listening addresses");
    info!("Node A listening addresses: {:?}", listening_addresses);

    let actual_tcp_addr = listening_addresses
        .iter()
        .find(|addr| {
            let s = addr.to_string();
            s.contains("/ip4/") && s.contains("/tcp/")
        })
        .expect("Should have a TCP address");

    // Extract the port from the actual address (e.g., "/ip4/127.0.0.1/tcp/12345")
    let port = actual_tcp_addr
        .iter()
        .find_map(|proto| {
            if let libp2p::multiaddr::Protocol::Tcp(port) = proto {
                Some(port)
            } else {
                None
            }
        })
        .expect("TCP address should have a port");

    // Construct a DNS address: /dns4/localhost/tcp/{port}
    let dns_addr: Multiaddr = format!("/dns4/localhost/tcp/{port}").parse().unwrap();
    info!(%dns_addr, "Connecting via DNS address");

    let connect_cmd = Command::ConnectToPeer {
        #[cfg(feature = "byos")]
        app_public_key: keypair_a.public(),
        #[cfg(not(feature = "byos"))]
        transport_id: keypair_a.public().to_peer_id(),
        addresses: vec![dns_addr.clone()],
    };
    command_b.send_command(connect_cmd).await;

    time::sleep(Duration::from_millis(2_000)).await;

    let (tx, rx) = oneshot::channel();
    let is_connected_query = QueryP2PStateCommand::IsConnected {
        #[cfg(feature = "byos")]
        app_public_key: keypair_a.public(),
        #[cfg(not(feature = "byos"))]
        transport_id: keypair_a.public().to_peer_id(),
        response_sender: tx,
    };
    command_b
        .send_command(Command::QueryP2PState(is_connected_query))
        .await;
    let is_connected = rx.await.expect("Failed to check connection status");

    assert!(
        is_connected,
        "Failed to establish connection via /dns4/localhost/tcp/{port}"
    );

    info!("DNS resolution test passed: connected via /dns4/localhost");

    cancel.cancel();
    let _ = join!(task_a, task_b);
}
