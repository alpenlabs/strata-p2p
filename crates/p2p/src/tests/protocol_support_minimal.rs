//! Tests for protocol support checking functionality.

use std::time::Duration;

use libp2p::{
    Transport,
    core::{muxing::StreamMuxerBox, upgrade::Version},
    identify::{Behaviour as Identify, Config},
    identity::Keypair,
    noise, yamux,
};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::info;

use super::common::init_tracing;
#[cfg(all(
    any(feature = "gossipsub", feature = "request-response"),
    not(feature = "byos")
))]
use crate::validator::DefaultP2PValidator;

#[cfg(feature = "mem-conn-limits-abs")]
const SIXTEEN_GIBIBYTES: usize = 16 * 1024 * 1024 * 1024;

#[cfg(feature = "request-response")]
#[derive(Debug, Clone, Default)]
struct TestCodec(std::marker::PhantomData<Vec<u8>>);

#[cfg(feature = "request-response")]
impl libp2p::request_response::Codec for TestCodec {
    type Protocol = libp2p::StreamProtocol;
    type Request = Vec<u8>;
    type Response = Vec<u8>;

    fn read_request<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _: &'life1 Self::Protocol,
        io: &'life2 mut T,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = std::io::Result<Self::Request>> + Send + 'async_trait>,
    >
    where
        T: futures::AsyncRead + Unpin + Send,
        T: 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            use futures::AsyncReadExt;
            let mut vec = Vec::new();
            io.read_to_end(&mut vec).await?;
            Ok(vec)
        })
    }

    fn read_response<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _: &'life1 Self::Protocol,
        io: &'life2 mut T,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::io::Result<Self::Response>> + Send + 'async_trait,
        >,
    >
    where
        T: futures::AsyncRead + Unpin + Send,
        T: 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            use futures::AsyncReadExt;
            let mut vec = Vec::new();
            io.read_to_end(&mut vec).await?;
            Ok(vec)
        })
    }

    fn write_request<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _: &'life1 Self::Protocol,
        io: &'life2 mut T,
        req: Self::Request,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + 'async_trait>,
    >
    where
        T: futures::AsyncWrite + Unpin + Send,
        T: 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            use futures::AsyncWriteExt;
            io.write_all(&req).await?;
            io.close().await?;
            Ok(())
        })
    }

    fn write_response<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _: &'life1 Self::Protocol,
        io: &'life2 mut T,
        res: Self::Response,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = std::io::Result<()>> + Send + 'async_trait>,
    >
    where
        T: futures::AsyncWrite + Unpin + Send,
        T: 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            use futures::AsyncWriteExt;
            io.write_all(&res).await?;
            io.close().await?;
            Ok(())
        })
    }
}

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

    let cancel = CancellationToken::new();
    #[cfg(feature = "byos")]
    let app_keypair = Keypair::generate_ed25519();
    let transport_keypair = Keypair::generate_ed25519();
    let listen_addr: libp2p::Multiaddr = "/memory/20000".parse()?;

    let config = crate::swarm::P2PConfig {
        #[cfg(feature = "byos")]
        app_public_key: app_keypair.public(),
        transport_keypair: transport_keypair.clone(),
        idle_connection_timeout: Duration::from_secs(30),
        max_retries: None,
        dial_timeout: None,
        general_timeout: None,
        connection_check_interval: None,
        listening_addrs: vec![listen_addr.clone()],
        connect_to: vec![],
        protocol_name: None,
        #[cfg(feature = "gossipsub")]
        gossipsub_topic: None,
        #[cfg(feature = "gossipsub")]
        gossipsub_max_transmit_size: None,
        #[cfg(feature = "gossipsub")]
        gossipsub_score_params: None,
        #[cfg(feature = "gossipsub")]
        gossipsub_score_thresholds: None,
        #[cfg(feature = "gossipsub")]
        gossip_event_buffer_size: None,
        #[cfg(feature = "gossipsub")]
        gossip_command_buffer_size: None,
        #[cfg(feature = "request-response")]
        channel_timeout: None,
        #[cfg(feature = "request-response")]
        req_resp_event_buffer_size: None,
        #[cfg(feature = "request-response")]
        request_max_bytes: None,
        #[cfg(feature = "request-response")]
        response_max_bytes: None,
        envelope_max_age: None,
        max_clock_skew: None,
        conn_limits: libp2p::connection_limits::ConnectionLimits::default()
            .with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        max_allowed_ram_used: SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        max_allowed_ram_used_percent: 1.0,
        command_buffer_size: None,
        handle_default_timeout: None,
        #[cfg(feature = "kad")]
        kad_protocol_name: None,
    };

    #[cfg(feature = "byos")]
    let signer = std::sync::Arc::new(super::common::MockApplicationSigner::new(
        app_keypair.clone(),
    ));
    let swarm = crate::swarm::with_inmemory_transport(
        &config,
        #[cfg(feature = "byos")]
        signer.clone(),
    )?;

    let p2p_config_result = crate::swarm::P2P::from_config(
        config,
        cancel.child_token(),
        swarm,
        #[cfg(feature = "byos")]
        vec![],
        #[cfg(feature = "gossipsub")]
        None, // gossip channel size
        #[cfg(all(
            feature = "byos",
            any(feature = "gossipsub", feature = "request-response")
        ))]
        signer.clone(),
        #[cfg(all(
            any(feature = "gossipsub", feature = "request-response"),
            not(feature = "byos")
        ))]
        Some(Box::new(DefaultP2PValidator)),
    )?;

    #[cfg(feature = "request-response")]
    let p2p = p2p_config_result.0;
    #[cfg(not(feature = "request-response"))]
    let p2p = p2p_config_result;

    let p2p_handle = tokio::spawn(async move {
        p2p.listen().await;
    });

    sleep(Duration::from_millis(100)).await;

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

    // Wait for connection attempt and potential disconnect
    sleep(Duration::from_secs(2)).await;

    // Check the results
    let (conn_established, conn_closed) = minimal_handle.await?;

    info!(
        "Connection established: {}, Connection closed: {}",
        conn_established, conn_closed
    );

    // We expect the connection to be established briefly but then closed due to protocol
    // mismatch - the main P2P node requires gossipsub but the minimal node doesn't support it
    assert!(
        conn_established,
        "Connection should be established initially"
    );
    assert!(
        conn_closed,
        "Connection should be closed due to missing gossipsub protocol"
    );

    cancel.cancel();
    let _ = p2p_handle.await;

    Ok(())
}

/// Test that peers without request-response support are disconnected.
/// This test runs when request-response feature is enabled (main node requires request-response).
#[cfg(all(feature = "request-response", feature = "gossipsub", feature = "byos"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_request_response_protocol_checking() -> anyhow::Result<()> {
    init_tracing();

    // Create a full P2P node with all features enabled
    let cancel = CancellationToken::new();
    let app_keypair = Keypair::generate_ed25519();
    let transport_keypair = Keypair::generate_ed25519();
    let listen_addr: libp2p::Multiaddr = "/memory/50000".parse()?;
    let topic = libp2p::gossipsub::Sha256Topic::new("test-topic");

    let config = crate::swarm::P2PConfig {
        app_public_key: app_keypair.public(),
        transport_keypair: transport_keypair.clone(),
        idle_connection_timeout: Duration::from_secs(30),
        max_retries: None,
        dial_timeout: None,
        general_timeout: None,
        connection_check_interval: None,
        listening_addrs: vec![listen_addr.clone()],
        connect_to: vec![],
        protocol_name: None,
        gossipsub_topic: Some("test-topic".to_string()),
        gossipsub_max_transmit_size: None,
        gossipsub_score_params: None,
        gossipsub_score_thresholds: None,
        gossip_event_buffer_size: None,
        gossip_command_buffer_size: None,
        channel_timeout: None,
        req_resp_event_buffer_size: None,
        request_max_bytes: None,
        response_max_bytes: None,
        envelope_max_age: None,
        max_clock_skew: None,
        conn_limits: libp2p::connection_limits::ConnectionLimits::default()
            .with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        max_allowed_ram_used: SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        max_allowed_ram_used_percent: 1.0,
        command_buffer_size: None,
        handle_default_timeout: None,
    };

    let signer = std::sync::Arc::new(super::common::MockApplicationSigner::new(
        app_keypair.clone(),
    ));
    let swarm = crate::swarm::with_inmemory_transport(&config, signer.clone())?;

    let p2p_config_result = crate::swarm::P2P::from_config(
        config,
        cancel.child_token(),
        swarm,
        vec![], // Allow any peers for this test
        None,   // gossip channel size
        signer.clone(),
        #[cfg(all(
            any(feature = "gossipsub", feature = "request-response"),
            not(feature = "byos")
        ))]
        Some(Box::new(DefaultP2PValidator)),
    )?;

    #[cfg(feature = "request-response")]
    let p2p = p2p_config_result.0;
    #[cfg(not(feature = "request-response"))]
    let p2p = p2p_config_result;

    let p2p_handle = tokio::spawn(async move {
        p2p.listen().await;
    });

    sleep(Duration::from_millis(100)).await;

    // Create a node with gossipsub and setup but NO request-response (will be disconnected)
    let no_req_resp_transport_keypair = Keypair::generate_ed25519();
    let no_req_resp_addr: libp2p::Multiaddr = "/memory/50001".parse()?;

    let gossipsub_setup_behaviour = GossipsubSetupOnlyBehaviour::new(
        "/strata",
        &no_req_resp_transport_keypair,
        &app_keypair.public(),
        &signer,
        &topic,
    );

    // Create swarm with gossipsub and setup but no request-response
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

    // Listen on the no-request-response address
    no_req_resp_swarm.listen_on(no_req_resp_addr)?;

    no_req_resp_swarm.dial(listen_addr)?;

    let mut connection_established = false;
    let mut connection_closed = false;

    // Start the no-request-response swarm and monitor events
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

    // Wait for connection attempt and potential disconnect
    sleep(Duration::from_secs(2)).await;

    // Check the results
    let (conn_established, conn_closed) = no_req_resp_handle.await?;

    info!(
        "Connection established: {}, Connection closed: {}",
        conn_established, conn_closed
    );

    // We expect the connection to be established briefly but then closed due to protocol mismatch
    // The main P2P node requires request-response (feature enabled) but the no-request-response
    // node doesn't support it
    assert!(
        conn_established,
        "Connection should be established initially"
    );
    assert!(
        conn_closed,
        "Connection should be closed due to missing request-response protocol"
    );

    cancel.cancel();
    let _ = p2p_handle.await;

    Ok(())
}

/// Test that peers without setup support are disconnected.
/// This test runs when byos feature is enabled (main node requires setup).
#[cfg(feature = "byos")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_setup_protocol_checking() -> anyhow::Result<()> {
    init_tracing();

    // Create a full P2P node with byos enabled (requires setup)
    let cancel = CancellationToken::new();
    let app_keypair = Keypair::generate_ed25519();
    let transport_keypair = Keypair::generate_ed25519();
    let listen_addr: libp2p::Multiaddr = "/memory/40000".parse()?;

    let config = crate::swarm::P2PConfig {
        app_public_key: app_keypair.public(),
        transport_keypair: transport_keypair.clone(),
        idle_connection_timeout: Duration::from_secs(30),
        max_retries: None,
        dial_timeout: None,
        general_timeout: None,
        connection_check_interval: None,
        listening_addrs: vec![listen_addr.clone()],
        connect_to: vec![],
        protocol_name: None,
        #[cfg(feature = "gossipsub")]
        gossipsub_topic: None,
        #[cfg(feature = "gossipsub")]
        gossipsub_max_transmit_size: None,
        #[cfg(feature = "gossipsub")]
        gossipsub_score_params: None,
        #[cfg(feature = "gossipsub")]
        gossipsub_score_thresholds: None,
        #[cfg(feature = "gossipsub")]
        gossip_event_buffer_size: None,
        #[cfg(feature = "gossipsub")]
        gossip_command_buffer_size: None,
        #[cfg(feature = "request-response")]
        channel_timeout: None,
        #[cfg(feature = "request-response")]
        req_resp_event_buffer_size: None,
        #[cfg(feature = "request-response")]
        request_max_bytes: None,
        #[cfg(feature = "request-response")]
        response_max_bytes: None,
        envelope_max_age: None,
        max_clock_skew: None,
        conn_limits: libp2p::connection_limits::ConnectionLimits::default()
            .with_max_established(Some(u32::MAX)),
        #[cfg(feature = "mem-conn-limits-abs")]
        max_allowed_ram_used: SIXTEEN_GIBIBYTES,
        #[cfg(feature = "mem-conn-limits-rel")]
        max_allowed_ram_used_percent: 1.0,
        command_buffer_size: None,
        handle_default_timeout: None,
    };

    let signer = std::sync::Arc::new(super::common::MockApplicationSigner::new(
        app_keypair.clone(),
    ));
    let swarm = crate::swarm::with_inmemory_transport(&config, signer.clone())?;

    let p2p_config_result = crate::swarm::P2P::from_config(
        config,
        cancel.child_token(),
        swarm,
        vec![], // Allow any peers for this test
        #[cfg(feature = "gossipsub")]
        None, // gossip channel size
        #[cfg(all(
            feature = "byos",
            any(feature = "gossipsub", feature = "request-response")
        ))]
        signer.clone(),
        #[cfg(all(
            any(feature = "gossipsub", feature = "request-response"),
            not(feature = "byos")
        ))]
        Some(Box::new(DefaultP2PValidator)),
    )?;

    #[cfg(feature = "request-response")]
    let p2p = p2p_config_result.0;
    #[cfg(not(feature = "request-response"))]
    let p2p = p2p_config_result;

    let p2p_handle = tokio::spawn(async move {
        p2p.listen().await;
    });

    sleep(Duration::from_millis(100)).await;

    // Create a minimal node that has NO setup protocol (will be disconnected)
    let minimal_transport_keypair = Keypair::generate_ed25519();
    let minimal_addr: libp2p::Multiaddr = "/memory/40001".parse()?;

    let minimal_behaviour = MinimalBehaviour::new("/strata", &minimal_transport_keypair);

    // Create swarm with minimal behavior (no setup protocol)
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

    // Wait for connection attempt and potential disconnect
    sleep(Duration::from_secs(2)).await;

    // Check the results
    let (conn_established, conn_closed) = minimal_handle.await?;

    info!(
        "Connection established: {}, Connection closed: {}",
        conn_established, conn_closed
    );
    // We expect the connection to be established briefly but then closed due to protocol mismatch
    // The main P2P node requires setup (byos feature enabled) but the minimal node doesn't support
    // it
    assert!(
        conn_established,
        "Connection should be established initially"
    );
    assert!(
        conn_closed,
        "Connection should be closed due to missing setup protocol"
    );

    cancel.cancel();
    let _ = p2p_handle.await;

    Ok(())
}
