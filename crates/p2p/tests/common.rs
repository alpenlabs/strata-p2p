use std::{sync::Arc, time::Duration};

use libp2p::{identity::secp256k1::Keypair as SecpKeypair, Multiaddr, PeerId};
use strata_p2p::{
    commands::{Command, UnsignedPublishMessage},
    swarm::{self, handle::P2PHandle, P2PConfig, P2P},
};
use strata_p2p_db::{sled::AsyncDB, GenesisInfoEntry, RepositoryExt};
use strata_p2p_types::OperatorPubKey;
use threadpool::ThreadPool;
use tokio_util::sync::CancellationToken;

use crate::mock_genesis_info;

pub struct Operator {
    pub p2p: P2P<(), AsyncDB>,
    pub handle: P2PHandle<()>,
    pub kp: SecpKeypair,
}

impl Operator {
    pub fn new(
        keypair: SecpKeypair,
        allowlist: Vec<PeerId>,
        connect_to: Vec<Multiaddr>,
        local_addr: Multiaddr,
        cancel: CancellationToken,
        signers_allowlist: Vec<OperatorPubKey>,
    ) -> anyhow::Result<Self> {
        let db = sled::Config::new().temporary(true).open()?;

        let config = P2PConfig {
            keypair: keypair.clone(),
            idle_connection_timeout: Duration::from_secs(30),
            listening_addr: local_addr,
            allowlist,
            connect_to,
            signers_allowlist,
        };

        let swarm = swarm::with_inmemory_transport(&config)?;
        let db = AsyncDB::new(ThreadPool::new(1), Arc::new(db));
        let (p2p, handle) = P2P::<(), AsyncDB>::from_config(config, cancel, db, swarm)?;

        Ok(Self {
            handle,
            p2p,
            kp: keypair,
        })
    }

    // Puts mock genesis info in db, since it is impossible
    // to do that externally without gossip.
    pub async fn new_with_genesis_info_entry_in_db(
        keypair: SecpKeypair,
        allowlist: Vec<PeerId>,
        connect_to: Vec<Multiaddr>,
        local_addr: Multiaddr,
        cancel: CancellationToken,
        signers_allowlist: Vec<OperatorPubKey>,
    ) -> anyhow::Result<Self> {
        let db = sled::Config::new().temporary(true).open()?;

        let config = P2PConfig {
            keypair: keypair.clone(),
            idle_connection_timeout: Duration::from_secs(30),
            listening_addr: local_addr,
            allowlist,
            connect_to,
            signers_allowlist,
        };

        let swarm = swarm::with_inmemory_transport(&config)?;
        let db = AsyncDB::new(ThreadPool::new(1), Arc::new(db));

        match mock_genesis_info(&keypair) {
            Command::PublishMessage(msg) => match msg.msg {
                UnsignedPublishMessage::GenesisInfo {
                    pre_stake_outpoint,
                    checkpoint_pubkeys,
                } => {
                    let entry = GenesisInfoEntry {
                        entry: (pre_stake_outpoint, checkpoint_pubkeys),
                        signature: msg.signature,
                        key: msg.key,
                    };
                    <AsyncDB as RepositoryExt<()>>::set_genesis_info_if_not_exists::<'_, '_>(
                        &db, entry,
                    )
                    .await?;
                }
                _ => unreachable!(),
            },
            _ => unreachable!(),
        }

        let (p2p, handle) = P2P::<(), AsyncDB>::from_config(config, cancel, db, swarm)?;

        Ok(Self {
            handle,
            p2p,
            kp: keypair,
        })
    }
}
