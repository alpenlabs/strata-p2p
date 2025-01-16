use std::{sync::Arc, time::Duration};

use libp2p::{identity::secp256k1::Keypair as SecpKeypair, Multiaddr, PeerId};
use snafu::ResultExt;
use strata_p2p::swarm::{self, handle::P2PHandle, P2PConfig, P2P};
use strata_p2p_db::sled::AsyncDB;
use tokio_util::sync::CancellationToken;

pub struct Operator {
    pub p2p: P2P<(), AsyncDB>,
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
        let db = sled::Config::new()
            .temporary(true)
            .open()
            .whatever_context("Failed to init DB")?;

        let config = P2PConfig {
            next_stage_timeout: Duration::from_secs(10),
            keypair,
            idle_connection_timeout: Duration::from_secs(30),
            listening_addr: local_addr,
            allowlist,
            connect_to,
        };

        let swarm = swarm::with_inmemory_transport(&config)
            .whatever_context("failed to initialize swarm")?;
        let db = AsyncDB::new(Default::default(), Arc::new(db));
        let (p2p, handle) = P2P::<(), AsyncDB>::from_config(config, cancel, db, swarm)
            .whatever_context("invalid p2p config")?;

        Ok(Self { handle, p2p })
    }
}
