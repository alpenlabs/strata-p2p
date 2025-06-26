//! Helper functions for the P2P tests.

use std::time::Duration;

use futures::future::join_all;
use libp2p::{
    Multiaddr, PeerId, build_multiaddr,
    identity::{Keypair, PublicKey, ed25519::Keypair as Ed25519Keypair},
};
use rand::Rng;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::swarm::{self, P2P, P2PConfig, handle::P2PHandle};

pub(crate) struct User {
    pub(crate) p2p: P2P,
    pub(crate) handle: P2PHandle,
    pub(crate) kp: Ed25519Keypair,
}

impl User {
    pub(crate) fn new(
        keypair: Ed25519Keypair,
        allowlist: Vec<PeerId>,
        connect_to: Vec<Multiaddr>,
        local_addr: Multiaddr,
        cancel: CancellationToken,
        signers_allowlist: Vec<PublicKey>,
    ) -> anyhow::Result<Self> {
        let config = P2PConfig {
            keypair: keypair.clone().into(),
            idle_connection_timeout: Duration::from_secs(30),
            max_retries: None,
            dial_timeout: None,
            general_timeout: None,
            connection_check_interval: None,
            listening_addr: local_addr,
            allowlist,
            connect_to,
            signers_allowlist,
        };

        let swarm = swarm::with_inmemory_transport(&config)?;
        let (p2p, handle) = P2P::from_config(config, cancel, swarm, None)?;

        Ok(Self {
            handle,
            p2p,
            kp: keypair,
        })
    }
}

/// Auxiliary structure to control operators from outside.
#[expect(dead_code)]
pub(crate) struct UserHandle {
    pub(crate) handle: P2PHandle,
    pub(crate) peer_id: PeerId,
    pub(crate) kp: Ed25519Keypair,
}

pub(crate) struct Setup {
    pub(crate) cancel: CancellationToken,
    pub(crate) operators: Vec<UserHandle>,
    pub(crate) tasks: TaskTracker,
}

impl Setup {
    /// Spawn `n` operators that are connected "all-to-all" with handles to them, task tracker
    /// to stop control async tasks they are spawned in.
    pub(crate) async fn all_to_all(n: usize) -> anyhow::Result<Self> {
        let (keypairs, peer_ids, multiaddresses) = Self::setup_keys_ids_addrs_of_n_operators(n);

        let cancel = CancellationToken::new();
        let mut operators = Vec::new();
        let signers_allowlist: Vec<PublicKey> = keypairs
            .clone()
            .into_iter()
            .map(|kp| kp.public().clone().into())
            .collect();

        for (idx, (keypair, addr)) in keypairs.iter().zip(&multiaddresses).enumerate() {
            let mut other_addrs = multiaddresses.clone();
            other_addrs.remove(idx);
            let mut other_peerids = peer_ids.clone();
            other_peerids.remove(idx);

            let operator = User::new(
                keypair.clone(),
                other_peerids,
                other_addrs,
                addr.clone(),
                cancel.child_token(),
                signers_allowlist.clone(),
            )?;

            operators.push(operator);
        }

        let (operators, tasks) = Self::start_operators(operators).await;

        Ok(Self {
            cancel,
            tasks,
            operators,
        })
    }

    /// Create `n` random keypairs, peer ids from them and sequential in-memory
    /// addresses.
    fn setup_keys_ids_addrs_of_n_operators(
        n: usize,
    ) -> (Vec<Ed25519Keypair>, Vec<PeerId>, Vec<libp2p::Multiaddr>) {
        let keypairs = (0..n)
            .map(|_| Ed25519Keypair::generate())
            .collect::<Vec<_>>();
        let peer_ids = keypairs
            .iter()
            .map(|key| PeerId::from_public_key(&Keypair::from(key.clone()).public()))
            .collect::<Vec<_>>();
        //
        // This allows multiple tests to not overlap multiaddresses in most cases. Unreliable but
        // works.
        let mut rng = rand::thread_rng();
        let mut multiaddr_base = rng.r#gen::<u16>();
        loop {
            if multiaddr_base > u16::max_value() - u16::try_from(n).unwrap() - 1 {
                multiaddr_base = rng.r#gen::<u16>();
            } else {
                break;
            };
        }

        let multiaddresses = (multiaddr_base
            ..(multiaddr_base + u16::try_from(keypairs.len() + 1).unwrap()))
            .map(|idx| build_multiaddr!(Memory(idx)))
            .collect::<Vec<_>>();
        (keypairs, peer_ids, multiaddresses)
    }

    /// Wait until all operators established connections with other operators,
    /// and then spawn [`P2P::listen`]s in separate tasks using [`TaskTracker`].
    async fn start_operators(mut operators: Vec<User>) -> (Vec<UserHandle>, TaskTracker) {
        // wait until all of them established connections and subscriptions
        join_all(
            operators
                .iter_mut()
                .map(|op| op.p2p.establish_connections())
                .collect::<Vec<_>>(),
        )
        .await;

        let mut levers = Vec::new();
        let tasks = TaskTracker::new();
        for operator in operators {
            let peer_id = operator.p2p.local_peer_id();
            tasks.spawn(operator.p2p.listen());

            levers.push(UserHandle {
                handle: operator.handle,
                peer_id,
                kp: operator.kp,
            });
        }

        tasks.close();
        (levers, tasks)
    }
}
