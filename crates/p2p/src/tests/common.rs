//! Helper functions for the P2P tests.

use std::{collections::HashSet, sync::Once, time::Duration};

use futures::future::join_all;
use libp2p::{
    Multiaddr, PeerId, build_multiaddr,
    identity::{Keypair, PublicKey},
};
use rand::Rng;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::debug;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(feature = "request-response")]
use crate::swarm::handle::ReqRespHandle;
use crate::{
    signer::ApplicationSigner,
    swarm::{
        self, P2P, P2PConfig,
        handle::{CommandHandle, GossipHandle},
    },
};

/// Only attempt to start tracing once
///
/// it is needed for supporting plain `cargo test`
pub(crate) fn init_tracing() {
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        tracing_subscriber::registry()
            .with(EnvFilter::from_default_env())
            .with(fmt::layer().with_file(true).with_line_number(true))
            .init();
    });
}

/// Mock ApplicationSigner for testing that stores the actual keypair
#[derive(Debug, Clone)]
pub(crate) struct MockApplicationSigner {
    // Store the actual keypair that corresponds to the app public key
    app_keypair: Keypair,
}

impl MockApplicationSigner {
    pub(crate) const fn new(app_keypair: Keypair) -> Self {
        Self { app_keypair }
    }
}

impl ApplicationSigner for MockApplicationSigner {
    fn sign(
        &self,
        message: &[u8],
        app_public_key: PublicKey,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        // Verify that the provided public key matches our stored keypair
        let our_public_key = self.app_keypair.public();
        if our_public_key.encode_protobuf() != app_public_key.encode_protobuf() {
            return Err("Public key mismatch: provided key doesn't match stored keypair".into());
        }

        // Sign with the stored keypair
        let signature = self.app_keypair.sign(message)?;
        Ok(signature)
    }
}

pub(crate) struct User {
    pub(crate) p2p: P2P<MockApplicationSigner>,
    pub(crate) gossip: GossipHandle,
    #[cfg(feature = "request-response")]
    pub(crate) reqresp: ReqRespHandle,
    pub(crate) command: CommandHandle,
    pub(crate) app_keypair: Keypair,
    pub(crate) transport_keypair: Keypair,
}

impl User {
    pub(crate) fn new(
        app_keypair: Keypair,
        transport_keypair: Keypair,
        connect_to: Vec<Multiaddr>,
        local_addr: Multiaddr,
        allow_list: HashSet<PublicKey>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Self> {
        debug!(%local_addr, "Creating new user with local address");

        let config = P2PConfig {
            app_public_key: app_keypair.public(),
            transport_keypair: transport_keypair.clone(),
            signer: MockApplicationSigner::new(app_keypair.clone()),
            idle_connection_timeout: Duration::from_secs(30),
            max_retries: None,
            dial_timeout: None,
            general_timeout: None,
            connection_check_interval: None,
            listening_addr: local_addr,
            allowlist: allow_list,
            connect_to,
            channel_timeout: None,
        };

        let swarm = swarm::with_inmemory_transport::<MockApplicationSigner>(&config)?;

        #[cfg(feature = "request-response")]
        let (p2p, reqresp) = P2P::from_config(config, cancel, swarm, None)?;
        #[cfg(not(feature = "request-response"))]
        let p2p = P2P::from_config(config, cancel, swarm, None)?;
        let gossip = p2p.new_gossip_handle();
        let command = p2p.new_command_handle();

        Ok(Self {
            p2p,
            gossip,
            #[cfg(feature = "request-response")]
            reqresp,
            command,
            app_keypair,
            transport_keypair,
        })
    }
}

/// Auxiliary structure to control users from outside.
#[expect(dead_code)]
pub(crate) struct UserHandle {
    pub(crate) gossip: GossipHandle,
    #[cfg(feature = "request-response")]
    pub(crate) reqresp: ReqRespHandle,
    pub(crate) command: CommandHandle,
    pub(crate) peer_id: PeerId,
    pub(crate) app_keypair: Keypair,
    pub(crate) transport_keypair: Keypair,
}

pub(crate) struct Setup {
    pub(crate) cancel: CancellationToken,
    pub(crate) user_handles: Vec<UserHandle>,
    pub(crate) tasks: TaskTracker,
}

impl Setup {
    /// Spawn `n` users that are connected "all-to-all" with handles to them, task tracker
    /// to stop control async tasks they are spawned in.
    pub(crate) async fn all_to_all(n: usize) -> anyhow::Result<Self> {
        let (app_keypairs, transport_keypairs, peer_ids, multiaddresses) =
            Self::setup_keys_ids_addrs_of_n_users(n);

        let cancel = CancellationToken::new();
        let mut users = Vec::new();

        for (idx, ((app_keypair, transport_keypair), addr)) in app_keypairs
            .iter()
            .zip(&transport_keypairs)
            .zip(&multiaddresses)
            .enumerate()
        {
            let mut other_addrs = multiaddresses.clone();
            other_addrs.remove(idx);
            let mut other_peerids = peer_ids.clone();
            other_peerids.remove(idx);
            let mut other_app_pk = app_keypairs
                .iter()
                .map(|kp| kp.public())
                .collect::<HashSet<_>>();
            other_app_pk.remove(&app_keypair.public());

            let user = User::new(
                app_keypair.clone(),
                transport_keypair.clone(),
                other_addrs,
                addr.clone(),
                other_app_pk,
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
    fn setup_keys_ids_addrs_of_n_users(
        n: usize,
    ) -> (
        Vec<Keypair>,
        Vec<Keypair>,
        Vec<PeerId>,
        Vec<libp2p::Multiaddr>,
    ) {
        let app_keypairs = (0..n)
            .map(|_| Keypair::generate_ed25519())
            .collect::<Vec<_>>();

        let transport_keypairs = (0..n)
            .map(|_| Keypair::generate_ed25519())
            .collect::<Vec<_>>();

        debug!(
            "len(app_keypairs):{}, len(transport_keypairs):{}",
            app_keypairs.len(),
            transport_keypairs.len()
        );

        let peer_ids = transport_keypairs
            .iter()
            .map(|key| PeerId::from_public_key(&key.clone().public()))
            .collect::<Vec<_>>();

        // This allows multiple tests to not overlap multiaddresses in most cases. Unreliable but
        // works.
        let mut rng = rand::rng();
        let mut multiaddr_base = rng.random::<u64>();
        loop {
            if multiaddr_base > u64::MAX - u64::try_from(n).unwrap() - 1 {
                multiaddr_base = rng.random::<u64>();
            } else {
                break;
            };
        }

        let multiaddresses = (multiaddr_base
            ..(multiaddr_base + u64::try_from(transport_keypairs.len()).unwrap()))
            .map(|idx| build_multiaddr!(Memory(idx)))
            .collect::<Vec<_>>();
        (app_keypairs, transport_keypairs, peer_ids, multiaddresses)
    }

    /// Wait until all users established connections with other users,
    /// and then spawn [`P2P::listen`]s in separate tasks using [`TaskTracker`].
    async fn start_users(mut users: Vec<User>) -> (Vec<UserHandle>, TaskTracker) {
        // wait until all of them established connections and subscriptions
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
            tasks.spawn(user.p2p.listen());
            levers.push(UserHandle {
                gossip: user.gossip,
                #[cfg(feature = "request-response")]
                reqresp: user.reqresp,
                command: user.command,
                peer_id,
                app_keypair: user.app_keypair,
                transport_keypair: user.transport_keypair,
            });
        }

        tasks.close();
        (levers, tasks)
    }
}
