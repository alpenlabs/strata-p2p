//! Helper functions for the P2P tests.

use std::{sync::Once, time::Duration};

use futures::future::join_all;
use libp2p::{
    build_multiaddr,
    identity::{Keypair, PublicKey},
    Multiaddr, PeerId,
};
use rand::Rng;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::debug;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[cfg(feature = "gossipsub")]
use crate::swarm::handle::GossipHandle;
#[cfg(feature = "request-response")]
use crate::swarm::handle::ReqRespHandle;
use crate::{
    signer::ApplicationSigner,
    swarm::{
        self,
        handle::{CommandHandle, GossipHandle},
        P2PConfig, P2P,
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
        _app_public_key: PublicKey,
    ) -> Result<[u8; 64], Box<dyn std::error::Error + Send + Sync>> {
        // Sign with the stored keypair
        let signature = self.app_keypair.sign(message)?;
        let sign_array: [u8; 64] = signature.try_into().unwrap();
        Ok(sign_array)
    }
}

pub(crate) struct User<S: ApplicationSigner = MockApplicationSigner> {
    pub(crate) p2p: P2P<S>,
    pub(crate) gossip: GossipHandle,
    #[cfg(feature = "request-response")]
    pub(crate) reqresp: ReqRespHandle,
    pub(crate) command: CommandHandle,
    pub(crate) app_keypair: Keypair,
    pub(crate) transport_keypair: Keypair,
}

impl<S: ApplicationSigner> User<S> {
    pub(crate) fn new(
        app_keypair: Keypair,
        transport_keypair: Keypair,
        connect_to: Vec<Multiaddr>,
        allowlist: Vec<PublicKey>,
        listening_addrs: Vec<Multiaddr>,
        cancel: CancellationToken,
        signer: S,
    ) -> anyhow::Result<Self> {
        debug!(
            ?listening_addrs,
            "Creating new user with listening addresses"
        );

        let config = P2PConfig {
            app_public_key: app_keypair.public(),
            transport_keypair: transport_keypair.clone(),
            idle_connection_timeout: Duration::from_secs(30),
            max_retries: None,
            dial_timeout: None,
            general_timeout: None,
            connection_check_interval: None,
            listening_addrs,
            connect_to,
            #[cfg(feature = "request-response")]
            channel_timeout: None,
            decay_factor: None,
            gossipsub_score_params: None,
            gossipsub_score_thresholds: None,
        };

        // Determine transport type based on the first listening address
        let use_inmemory = config
            .listening_addrs
            .first()
            .map(|addr| addr.to_string().starts_with("/memory/"))
            .unwrap_or(false);

        let swarm = if use_inmemory {
            swarm::with_inmemory_transport(&config, signer.clone())?
        } else {
            swarm::with_default_transport(&config, signer.clone())?
        };

        #[cfg(feature = "request-response")]
        let (p2p, reqresp) =
            P2P::from_config(config, cancel, swarm, allowlist, None, signer.clone())?;
        #[cfg(not(feature = "request-response"))]
        let p2p = P2P::from_config(config, cancel, swarm, None, None, signer.clone())?;
        #[cfg(feature = "gossipsub")]
        let gossip = p2p.new_gossip_handle();
        let command = p2p.new_command_handle();

        Ok(Self {
            p2p,
            #[cfg(feature = "gossipsub")]
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
pub(crate) struct UserHandle {
    pub(crate) peer_id: PeerId,
    #[cfg(feature = "gossipsub")]
    pub(crate) gossip: GossipHandle,
    #[cfg(feature = "request-response")]
    pub(crate) reqresp: ReqRespHandle,
    pub(crate) command: CommandHandle,
    pub(crate) app_keypair: Keypair,
}

pub(crate) struct Setup {
    pub(crate) cancel: CancellationToken,
    pub(crate) user_handles: Vec<UserHandle>,
    pub(crate) tasks: TaskTracker,
}

pub(crate) struct SetupInitialData {
    pub(crate) app_keypairs: Vec<Keypair>,
    pub(crate) transport_keypairs: Vec<Keypair>,
    pub(crate) peer_ids: Vec<PeerId>,
    pub(crate) multiaddresses: Vec<libp2p::Multiaddr>,
}

impl Setup {
    /// Spawn `n` users that are connected "all-to-all" with handles to them, task tracker
    /// to stop control async tasks they are spawned in.
    pub(crate) async fn all_to_all(n: usize) -> anyhow::Result<Self> {
        let SetupInitialData {
            app_keypairs,
            transport_keypairs,
            peer_ids,
            multiaddresses,
        } = Self::setup_keys_ids_addrs_of_n_users(n);

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
                .collect::<Vec<_>>();
            other_app_pk.remove(idx);

            let user = User::new(
                app_keypair.clone(),
                transport_keypair.clone(),
                other_addrs,
                other_app_pk,
                vec![addr.clone()],
                cancel.child_token(),
                MockApplicationSigner {
                    app_keypair: app_keypair.clone(),
                },
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

    /// Create `n` random keypairs, transport ids from them and sequential in-memory
    /// addresses.
    fn setup_keys_ids_addrs_of_n_users(n: usize) -> SetupInitialData {
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

        SetupInitialData {
            app_keypairs,
            transport_keypairs,
            peer_ids,
            multiaddresses,
        }
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
                #[cfg(feature = "gossipsub")]
                gossip: user.gossip,
                #[cfg(feature = "request-response")]
                reqresp: user.reqresp,
                command: user.command,
                peer_id,
                app_keypair: user.app_keypair,
            });
        }

        tasks.close();
        (levers, tasks)
    }
}
