//! Tests for protocol support checking functionality.

#[cfg(any(feature = "gossipsub", feature = "byos"))]
use std::time::Duration;

#[cfg(any(feature = "gossipsub", feature = "byos"))]
use libp2p::{
    Transport,
    core::{muxing::StreamMuxerBox, upgrade::Version},
    identify::{Behaviour as Identify, Config},
    identity::Keypair,
    noise, yamux,
};
#[cfg(any(feature = "gossipsub", feature = "byos"))]
use tokio::time::sleep;
#[cfg(any(feature = "gossipsub", feature = "byos"))]
use tracing::info;

#[cfg(any(feature = "gossipsub", feature = "byos"))]
use super::common::{Setup, init_tracing};

#[cfg(any(feature = "gossipsub", feature = "byos"))]
#[derive(libp2p::swarm::NetworkBehaviour)]
struct MinimalBehaviour {
    /// Identification of peers, address to connect to, public keys, etc.
    pub identify: Identify,
}

#[cfg(any(feature = "gossipsub", feature = "byos"))]
impl MinimalBehaviour {
    fn new(protocol_name: &'static str, transport_keypair: &Keypair) -> Self {
        Self {
            identify: Identify::new(Config::new(
                protocol_name.to_string(),
                transport_keypair.public(),
            )),
        }
    }
}

#[cfg(all(feature = "gossipsub", feature = "byos", feature = "request-response"))]
#[derive(libp2p::swarm::NetworkBehaviour)]
struct GossipsubSetupOnlyBehaviour {
    /// Identification of peers, address to connect to, public keys, etc.
    pub identify: Identify,

    /// Gossipsub - pub/sub model for messages distribution.
    pub gossipsub: libp2p::gossipsub::Behaviour<
        libp2p::gossipsub::IdentityTransform,
        libp2p::gossipsub::WhitelistSubscriptionFilter,
    >,

    /// Setup protocol for application key exchange.
    pub setup: crate::swarm::setup::behavior::SetupBehaviour,
}

#[cfg(all(feature = "gossipsub", feature = "byos", feature = "request-response"))]
impl GossipsubSetupOnlyBehaviour {
    fn new(
        protocol_name: &'static str,
        transport_keypair: &Keypair,
        app_public_key: &libp2p::identity::PublicKey,
        signer: &std::sync::Arc<super::common::MockApplicationSigner>,
        topic: &libp2p::gossipsub::Sha256Topic,
    ) -> Self {
        use libp2p::gossipsub::{
            Behaviour, MessageAuthenticity, ValidationMode, WhitelistSubscriptionFilter,
        };

        let mut filter = std::collections::HashSet::new();
        filter.insert(topic.hash());

        let gossipsub = Behaviour::new_with_subscription_filter(
            MessageAuthenticity::Author(libp2p::PeerId::from_public_key(
                &transport_keypair.public().clone(),
            )),
            libp2p::gossipsub::ConfigBuilder::default()
                .validation_mode(ValidationMode::Permissive)
                .validate_messages()
                .max_transmit_size(524288)
                .idontwant_on_publish(true)
                .build()
                .expect("gossipsub config should be valid"),
            None,
            WhitelistSubscriptionFilter(filter),
        )
        .expect("gossipsub should be created");

        let setup = crate::swarm::setup::behavior::SetupBehaviour::new(
            app_public_key.clone(),
            transport_keypair.public().to_peer_id(),
            signer.clone(),
        );

        Self {
            identify: Identify::new(Config::new(
                protocol_name.to_string(),
                transport_keypair.public(),
            )),
            gossipsub,
            setup,
        }
    }
}

#[cfg(feature = "gossipsub")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_gossipsub_protocol_checking() -> anyhow::Result<()> {
    init_tracing();

    let Setup {
        cancel,
        user_handles,
        ..
    } = Setup::all_to_all(1).await?;

    let full_node_handle = &user_handles[0];

    sleep(Duration::from_millis(100)).await;

    let (addr_sender, addr_receiver) = tokio::sync::oneshot::channel();
    full_node_handle
        .command
        .send_command(
            crate::commands::QueryP2PStateCommand::GetMyListeningAddresses {
                response_sender: addr_sender,
            },
        )
        .await;

    let listening_addrs = addr_receiver.await?;
    let listen_addr = listening_addrs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No listening addresses found"))?;

    let minimal_transport_keypair = Keypair::generate_ed25519();
    let minimal_addr: libp2p::Multiaddr = "/memory/20001".parse()?;

    let minimal_behaviour = MinimalBehaviour::new("/strata", &minimal_transport_keypair);

    let mut minimal_swarm = libp2p::SwarmBuilder::with_existing_identity(minimal_transport_keypair)
        .with_tokio()
        .with_other_transport(|keypair| {
            libp2p::core::transport::MemoryTransport::new()
                .upgrade(Version::V1)
                .authenticate(noise::Config::new(keypair).unwrap())
                .multiplex(yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        })
        .unwrap()
        .with_behaviour(|_| minimal_behaviour)
        .unwrap()
        .build();

    minimal_swarm.listen_on(minimal_addr)?;

    minimal_swarm.dial(listen_addr)?;

    let mut connection_established = false;
    let mut connection_closed = false;

    let minimal_handle = tokio::spawn(async move {
        use futures::StreamExt;
        use libp2p::swarm::SwarmEvent;

        let mut swarm = minimal_swarm;
        while let Some(event) = swarm.next().await {
            match event {
                SwarmEvent::ConnectionEstablished { .. } => {
                    info!("Minimal node connected to full P2P node");
                    connection_established = true;
                }
                SwarmEvent::ConnectionClosed { .. } => {
                    info!(
                        "Minimal node connection closed (expected due to missing gossipsub protocol)"
                    );
                    connection_closed = true;
                    break; // Exit the loop when connection is closed
                }
                _ => {}
            }
        }
        (connection_established, connection_closed)
    });

    sleep(Duration::from_secs(2)).await;

    let (conn_established, conn_closed) = minimal_handle.await?;

    info!(
        "Connection established: {}, Connection closed: {}",
        conn_established, conn_closed
    );

    assert!(
        conn_established,
        "Connection should be established initially"
    );
    assert!(
        conn_closed,
        "Connection should be closed due to missing gossipsub protocol"
    );

    cancel.cancel();

    Ok(())
}

/// Test that peers without request-response support are disconnected.
/// This test runs when request-response feature is enabled (main node requires request-response).
#[cfg(all(feature = "request-response", feature = "gossipsub", feature = "byos"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_request_response_protocol_checking() -> anyhow::Result<()> {
    init_tracing();

    let topic = libp2p::gossipsub::Sha256Topic::new("test-topic");

    let Setup {
        cancel,
        user_handles,
        ..
    } = Setup::all_to_all(1).await?;

    let full_node_handle = &user_handles[0];

    sleep(Duration::from_millis(100)).await;

    let (addr_sender, addr_receiver) = tokio::sync::oneshot::channel();
    full_node_handle
        .command
        .send_command(
            crate::commands::QueryP2PStateCommand::GetMyListeningAddresses {
                response_sender: addr_sender,
            },
        )
        .await;

    let listening_addrs = addr_receiver.await?;
    let listen_addr = listening_addrs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No listening addresses found"))?;

    let signer = std::sync::Arc::new(super::common::MockApplicationSigner::new(
        full_node_handle.app_keypair.clone(),
    ));

    let no_req_resp_transport_keypair = Keypair::generate_ed25519();
    let no_req_resp_addr: libp2p::Multiaddr = "/memory/50001".parse()?;

    let gossipsub_setup_behaviour = GossipsubSetupOnlyBehaviour::new(
        "/strata",
        &no_req_resp_transport_keypair,
        &full_node_handle.app_keypair.public(),
        &signer,
        &topic,
    );

    let mut no_req_resp_swarm =
        libp2p::SwarmBuilder::with_existing_identity(no_req_resp_transport_keypair)
            .with_tokio()
            .with_other_transport(|keypair| {
                libp2p::core::transport::MemoryTransport::new()
                    .upgrade(Version::V1)
                    .authenticate(noise::Config::new(keypair).unwrap())
                    .multiplex(yamux::Config::default())
                    .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
            })
            .unwrap()
            .with_behaviour(|_| gossipsub_setup_behaviour)
            .unwrap()
            .build();

    no_req_resp_swarm.listen_on(no_req_resp_addr)?;

    no_req_resp_swarm.dial(listen_addr)?;

    let mut connection_established = false;
    let mut connection_closed = false;

    let no_req_resp_handle = tokio::spawn(async move {
        use futures::StreamExt;
        use libp2p::swarm::SwarmEvent;

        let mut swarm = no_req_resp_swarm;
        while let Some(event) = swarm.next().await {
            match event {
                SwarmEvent::ConnectionEstablished { .. } => {
                    info!("No-request-response node connected to full P2P node");
                    connection_established = true;
                }
                SwarmEvent::ConnectionClosed { .. } => {
                    info!(
                        "No-request-response node connection closed (expected due to missing request-response protocol)"
                    );
                    connection_closed = true;
                    break;
                }
                _ => {}
            }
        }
        (connection_established, connection_closed)
    });

    sleep(Duration::from_secs(2)).await;

    let (conn_established, conn_closed) = no_req_resp_handle.await?;

    info!(
        "Connection established: {}, Connection closed: {}",
        conn_established, conn_closed
    );

    assert!(
        conn_established,
        "Connection should be established initially"
    );
    assert!(
        conn_closed,
        "Connection should be closed due to missing request-response protocol"
    );

    cancel.cancel();

    Ok(())
}

/// Test that peers without setup support are disconnected.
/// This test runs when byos feature is enabled (main node requires setup).
#[cfg(feature = "byos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_setup_protocol_checking() -> anyhow::Result<()> {
    init_tracing();

    let Setup {
        cancel,
        user_handles,
        ..
    } = Setup::all_to_all(1).await?;

    let full_node_handle = &user_handles[0];

    sleep(Duration::from_millis(100)).await;

    let (addr_sender, addr_receiver) = tokio::sync::oneshot::channel();
    full_node_handle
        .command
        .send_command(
            crate::commands::QueryP2PStateCommand::GetMyListeningAddresses {
                response_sender: addr_sender,
            },
        )
        .await;

    let listening_addrs = addr_receiver.await?;
    let listen_addr = listening_addrs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No listening addresses found"))?;

    let minimal_transport_keypair = Keypair::generate_ed25519();
    let minimal_addr: libp2p::Multiaddr = "/memory/40001".parse()?;

    let minimal_behaviour = MinimalBehaviour::new("/strata", &minimal_transport_keypair);

    let mut minimal_swarm = libp2p::SwarmBuilder::with_existing_identity(minimal_transport_keypair)
        .with_tokio()
        .with_other_transport(|keypair| {
            libp2p::core::transport::MemoryTransport::new()
                .upgrade(Version::V1)
                .authenticate(noise::Config::new(keypair).unwrap())
                .multiplex(yamux::Config::default())
                .map(|(p, c), _| (p, StreamMuxerBox::new(c)))
        })
        .unwrap()
        .with_behaviour(|_| minimal_behaviour)
        .unwrap()
        .build();

    minimal_swarm.listen_on(minimal_addr)?;

    minimal_swarm.dial(listen_addr)?;

    let mut connection_established = false;
    let mut connection_closed = false;

    let minimal_handle = tokio::spawn(async move {
        use futures::StreamExt;
        use libp2p::swarm::SwarmEvent;

        let mut swarm = minimal_swarm;
        while let Some(event) = swarm.next().await {
            match event {
                SwarmEvent::ConnectionEstablished { .. } => {
                    info!("Minimal node connected to full P2P node");
                    connection_established = true;
                }
                SwarmEvent::ConnectionClosed { .. } => {
                    info!(
                        "Minimal node connection closed (expected due to missing setup protocol)"
                    );
                    connection_closed = true;
                    break;
                }
                _ => {}
            }
        }
        (connection_established, connection_closed)
    });

    sleep(Duration::from_secs(2)).await;

    let (conn_established, conn_closed) = minimal_handle.await?;

    info!(
        "Connection established: {}, Connection closed: {}",
        conn_established, conn_closed
    );

    assert!(
        conn_established,
        "Connection should be established initially"
    );
    assert!(
        conn_closed,
        "Connection should be closed due to missing setup protocol"
    );

    cancel.cancel();

    Ok(())
}
