//! Helper functions for the P2P tests.

use std::time::Duration;

use futures::future::join_all;
use libp2p::{Multiaddr, PeerId, StreamProtocol, build_multiaddr, identity::Keypair};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{debug, trace};

use crate::swarm::{self, P2P, P2PConfig, handle::P2PHandle};

pub(crate) struct User {
    pub(crate) p2p: P2P,
    pub(crate) handle: P2PHandle,
    pub(crate) kp: Keypair,
}

impl User {
    pub(crate) fn new(
        keypair: Keypair,
        blacklist: Vec<PeerId>,
        connect_to: Vec<Multiaddr>,
        local_addr: Multiaddr,
        cancel: CancellationToken,
    ) -> anyhow::Result<Self> {
        debug!("In User::new: local_addr: {}", local_addr.clone());

        let config = P2PConfig {
            keypair: keypair.clone(),
            idle_connection_timeout: Duration::from_secs(30),
            max_retries: None,
            dial_timeout: None,
            general_timeout: None,
            connection_check_interval: None,
            listening_addr: local_addr,
            blacklist,
            connect_to,
            kad_protocol_name: Some(StreamProtocol::new("/ipfs/kad_strata-p2p/0.0.1")),
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

/// Auxiliary structure to control users from outside.
#[expect(dead_code)]
pub(crate) struct UserHandle {
    pub(crate) handle: P2PHandle,
    pub(crate) peer_id: PeerId,
    pub(crate) kp: Keypair,
}

pub(crate) struct Setup {
    pub(crate) cancel: CancellationToken,
    pub(crate) user_handles: Vec<UserHandle>,
    pub(crate) tasks: TaskTracker,
}

impl Setup {
    /// Spawn `n` users that are connected "all-to-all" with handles to them, task tracker
    /// to stop control async tasks they are spawned in.
    pub(crate) async fn all_to_all(n: usize, offset: u64) -> anyhow::Result<Self> {
        let (keypairs, _peer_ids, multiaddresses) =
            Self::setup_keys_ids_addrs_of_n_users(n, offset);

        let cancel = CancellationToken::new();
        let mut users = Vec::new();

        for (idx, (keypair, addr)) in keypairs.iter().zip(&multiaddresses).enumerate() {
            let mut other_addrs = multiaddresses.clone();
            other_addrs.remove(idx);
            // let mut other_peerids = peer_ids.clone();
            // other_peerids.remove(idx);

            let user = User::new(
                keypair.clone(),
                Vec::new(),
                other_addrs,
                addr.clone(),
                cancel.child_token(),
            )?;

            users.push(user);
        }

        let (users, tasks) = Self::start_users(users).await;

        Ok(Self {
            cancel,
            tasks,
            user_handles: users,
        })
    }

    /// Create `n` random keypairs, peer ids from them and sequential in-memory
    /// addresses.
    /// If offset is 1234567890, then addresses will be /memory/1234567890 for n=1, for n=2
    /// /memory/1234567890 and /memory/1234567891 and so on.
    fn setup_keys_ids_addrs_of_n_users(
        n: usize,
        offset: u64,
    ) -> (Vec<Keypair>, Vec<PeerId>, Vec<libp2p::Multiaddr>) {
        let keypairs = (0..n)
            .map(|_| Keypair::generate_ed25519())
            .collect::<Vec<_>>();

        debug!("len(keypairs):{}", keypairs.len());

        let peer_ids = keypairs
            .iter()
            .map(|key| PeerId::from_public_key(&key.clone().public()))
            .collect::<Vec<_>>();

        let multiaddresses = (offset..(offset + u64::try_from(keypairs.len()).unwrap()))
            .map(|idx| build_multiaddr!(Memory(idx)))
            .collect::<Vec<_>>();
        (keypairs, peer_ids, multiaddresses)
    }

    /// Wait until all users established connections with other users,
    /// and then spawn [`P2P::listen`]s in separate tasks using [`TaskTracker`].
    async fn start_users(mut users: Vec<User>) -> (Vec<UserHandle>, TaskTracker) {
        trace!("waiting until all of them established connections and subscriptions");
        join_all(
            users
                .iter_mut()
                .map(|op| op.p2p.establish_connections())
                .collect::<Vec<_>>(),
        )
        .await;

        let mut levers = Vec::new();
        let tasks = TaskTracker::new();
        for user in users {
            let peer_id = user.p2p.local_peer_id();
            trace!("Spawning task to listen for {peer_id}");
            tasks.spawn(user.p2p.listen());

            levers.push(UserHandle {
                handle: user.handle,
                peer_id,
                kp: user.kp,
            });
        }

        tasks.close();
        (levers, tasks)
    }
}
