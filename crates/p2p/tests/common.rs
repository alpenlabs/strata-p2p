use std::time::Duration;

use libp2p::{identity::secp256k1::Keypair as SecpKeypair, Multiaddr, PeerId};
use snafu::ResultExt;
use strata_p2p::swarm::{handle::P2PHandle, P2PConfig, P2P};
use tokio_util::sync::CancellationToken;

pub struct Operator {
    pub p2p: P2P<(), sled::Db>,
    pub handle: P2PHandle<()>,
}

impl Operator {
    pub fn new(
        keypair: SecpKeypair,
        allowlist: Vec<PeerId>,
        connect_to: Vec<Multiaddr>,
        local_addr: Multiaddr,
        cancel: CancellationToken,
    ) -> Result<Self, snafu::Whatever> {
        let config = P2PConfig {
            next_stage_timeout: Duration::from_secs(10),
            keypair,
            idle_connection_timeout: Duration::from_secs(30),
            listening_addr: local_addr,
            allowlist,
            connect_to,
            database_path: "in-memory".into(),
        };

        let (p2p, handle) = P2P::<(), sled::Db>::from_config(config, cancel, true)
            .whatever_context("invalid p2p config")?;

        Ok(Self { handle, p2p })
    }
}
