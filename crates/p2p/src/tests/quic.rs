//! Test QUIC and TCP connectivity on IPv4 and IPv6.

use std::time::Duration;

use futures::StreamExt;
use libp2p::{
    Multiaddr,
    identity::Keypair,
    swarm::{Swarm, SwarmEvent, dial_opts::DialOpts},
};
use tokio::time::timeout;
use tracing::info;

use crate::swarm::{Behaviour, P2PConfig, with_default_transport};

const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const WAIT_CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);

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

    let mut swarm_a = {
        let cfg = P2PConfig {
            keypair: keypair_a.clone(),
            idle_connection_timeout: Duration::from_secs(10),
            max_retries: Some(3),
            dial_timeout: Some(Duration::from_secs(2)),
            general_timeout: Some(Duration::from_secs(2)),
            connection_check_interval: Some(Duration::from_millis(100)),
            listening_addr: tcp4_base.clone(),
            allowlist: vec![peer_id_b],
            connect_to: vec![],
        };
        let mut s = with_default_transport(&cfg).expect("build listener swarm");
        s.listen_on(tcp4_base.clone()).unwrap();
        s.listen_on(quic4_base.clone()).unwrap();
        s.listen_on(tcp6_base.clone()).unwrap();
        s.listen_on(quic6_base.clone()).unwrap();
        s
    };

    let mut swarm_b = {
        let cfg = P2PConfig {
            keypair: keypair_b.clone(),
            idle_connection_timeout: Duration::from_secs(10),
            max_retries: Some(3),
            dial_timeout: Some(Duration::from_secs(2)),
            general_timeout: Some(Duration::from_secs(2)),
            connection_check_interval: Some(Duration::from_millis(100)),
            listening_addr: tcp4_base.clone(),
            allowlist: vec![peer_id_a],
            connect_to: vec![],
        };
        with_default_transport(&cfg).expect("build dialer swarm")
    };

    let (tcp4_addr, quic4_addr, tcp6_addr, quic6_addr) = {
        let mut tcp4_addr = None;
        let mut quic4_addr = None;
        let mut tcp6_addr = None;
        let mut quic6_addr = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            if let Ok(Some(SwarmEvent::NewListenAddr { address, .. })) =
                timeout(deadline - tokio::time::Instant::now(), swarm_a.next()).await
            {
                let s = address.to_string();
                if s.contains("/ip4/") && s.contains("/tcp/") {
                    tcp4_addr = Some(address.clone());
                }
                if s.contains("/ip4/") && s.contains("/quic-v1") {
                    quic4_addr = Some(address.clone());
                }
                if s.contains("/ip6/") && s.contains("/tcp/") {
                    tcp6_addr = Some(address.clone());
                }
                if s.contains("/ip6/") && s.contains("/quic-v1") {
                    quic6_addr = Some(address.clone());
                }
                if tcp4_addr.is_some()
                    && quic4_addr.is_some()
                    && tcp6_addr.is_some()
                    && quic6_addr.is_some()
                {
                    break;
                }
            }
        }
        let tcp4_addr = tcp4_addr.expect("listener did not report a TCP/IPv4 address");
        let quic4_addr = quic4_addr.expect("listener did not report a QUIC/IPv4 address");
        let tcp6_addr = tcp6_addr.expect("listener did not report a TCP/IPv6 address");
        let quic6_addr = quic6_addr.expect("listener did not report a QUIC/IPv6 address");
        info!(%tcp4_addr, %quic4_addr, %tcp6_addr, %quic6_addr, "Listener addresses ready");
        (tcp4_addr, quic4_addr, tcp6_addr, quic6_addr)
    };

    async fn wait_connection(
        swarm1: &mut Swarm<Behaviour>,
        swarm2: &mut Swarm<Behaviour>,
        timeout_dur: Duration,
    ) {
        let mut got_event1 = false;
        let mut got_event2 = false;

        timeout(timeout_dur, async {
            loop {
                tokio::select! {
                    event = swarm1.next() => {
                        if let Some(ev) = event {
                            info!(event = ?ev, "swarm1 event");
                            if let SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } = ev {
                                let addr = endpoint.get_remote_address();
                                info!(%peer_id, %addr, "Connection established on swarm1");
                                got_event1 = true;
                            }
                        }
                    }
                    event = swarm2.next() => {
                        if let Some(ev) = event {
                            info!(event = ?ev, "swarm2 event");
                            if let SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } = ev {
                                let addr = endpoint.get_remote_address();
                                info!(%peer_id, %addr, "Connection established on swarm2");
                                got_event2 = true;
                            }
                        }
                    }
                }
                if got_event1 && got_event2 {
                    break;
                }
            }
        })
        .await
        .expect("timed out waiting for connection");
    }

    for (label, addr) in [
        ("TCP/IPv4", tcp4_addr.clone()),
        ("QUIC/IPv4", quic4_addr.clone()),
        ("TCP/IPv6", tcp6_addr.clone()),
        ("QUIC/IPv6", quic6_addr.clone()),
    ] {
        info!(%label, %addr, "Dialing");
        let dial_opts = DialOpts::unknown_peer_id().address(addr.clone()).build();
        swarm_b
            .dial(dial_opts)
            .unwrap_or_else(|_| panic!("dial {label}"));
        wait_connection(&mut swarm_a, &mut swarm_b, WAIT_CONNECTION_TIMEOUT).await;
        let peer_id = *swarm_a.local_peer_id();
        let _ = swarm_b.disconnect_peer_id(peer_id);
        tracing::info!(%label, "Disconnect requested");

        let disconnect_deadline = tokio::time::Instant::now() + DISCONNECT_TIMEOUT;
        while swarm_b.is_connected(&peer_id) && tokio::time::Instant::now() < disconnect_deadline {
            swarm_b.next().await;
        }
        info!(connected = swarm_b.is_connected(&peer_id), %label, "Connection state after disconnect");
    }

    info!("All QUIC and TCP connections (IPv4 and IPv6) succeeded");
}
