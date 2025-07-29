//! Helper functions for the P2P tests.

use std::{sync::Once, time::Duration};

use futures::future::join_all;
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, build_multiaddr,
    identity::{Keypair, PublicKey},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{debug, trace};
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

// this is here in one place.
pub(crate) const MULTIADDR_MEMORY_ID_OFFSET_GOSSIP_BASIC: u64 = 120;
pub(crate) const MULTIADDR_MEMORY_ID_OFFSET_TEST_IS_CONNECTED: u64 = 150;
pub(crate) const MULTIADDR_MEMORY_ID_OFFSET_GOSSIP_NEW_USER: u64 = 200;

#[cfg(feature = "request-response")]
pub(crate) const MULTIADDR_MEMORY_ID_OFFSET_REQUEST_RESPONSE_BASIC: u64 = 300;

pub(crate) const MULTIADDR_MEMORY_ID_OFFSET_TEST_MANUALLY_GET_ALL_PEERS: u64 = 500;
pub(crate) const MULTIADDR_MEMORY_ID_OFFSET_TEST_CONNECTION_BY_APP_PUBLIC_KEY: u64 = 600;
pub(crate) const MULTIADDR_MEMORY_ID_OFFSET_TEST_SETUP_WITH_INVALID_SIGNATURE: u64 = 700;

#[cfg(all(feature = "request-response", feature = "kademlia"))]
pub(crate) const MULTIADDR_MEMORY_ID_OFFSET_TEST_DHT_RECORD: u64 = 1000;

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
        local_addr: Multiaddr,
        allowlist: Vec<PublicKey>,
        cancel: CancellationToken,
        signer: S,
    ) -> anyhow::Result<Self> {
        debug!(%local_addr, "Creating new user with local address");

        let config = P2PConfig {
            app_public_key: app_keypair.public(),
            transport_keypair: transport_keypair.clone(),
            idle_connection_timeout: Duration::from_secs(30),
            max_retries: None,
            dial_timeout: None,
            general_timeout: None,
            connection_check_interval: None,
            listening_addr: local_addr,
            connect_to,
            kad_protocol_name: Some(StreamProtocol::new("/ipfs/kad_strata-p2p/0.0.1")),
            #[cfg(feature = "request-response")]
            channel_timeout: None,
            kademlia_threshold: 2,
        };

        let swarm = swarm::with_inmemory_transport::<S>(&config, signer.clone())?;

        #[cfg(feature = "request-response")]
        let (p2p, reqresp) = P2P::from_config(config, cancel, swarm, allowlist, None, signer)?;
        #[cfg(not(feature = "request-response"))]
        let p2p = P2P::from_config(config, cancel, swarm, allowlist, None, signer)?;
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

pub(crate) struct SetupInitialData {
    pub(crate) app_keypairs: Vec<Keypair>,
    pub(crate) transport_keypairs: Vec<Keypair>,
    pub(crate) multiaddresses: Vec<libp2p::Multiaddr>,
}

impl Setup {
    /// Spawn `n` users that are connected "all-to-all" with handles to them, task tracker
    /// to stop control async tasks they are spawned in.
    pub(crate) async fn all_to_all(n: usize, offset: u64) -> anyhow::Result<Self> {
        let SetupInitialData {
            app_keypairs,
            transport_keypairs,
            multiaddresses,
        } = Self::setup_keys_ids_addrs_of_n_users(n, offset);

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
            let mut other_app_pk = app_keypairs
                .iter()
                .map(|kp| kp.public())
                .collect::<Vec<_>>();
            other_app_pk.remove(idx);

            let user = User::new(
                app_keypair.clone(),
                transport_keypair.clone(),
                other_addrs,
                addr.clone(),
                other_app_pk,
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

    pub(crate) async fn all_to_all_limited(
        n: usize,
        init_limit: usize,
        offset: u64,
    ) -> anyhow::Result<Self> {
        let SetupInitialData {
            app_keypairs,
            transport_keypairs,
            multiaddresses,
        } = Self::setup_keys_ids_addrs_of_n_users(n, offset);

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
            let limited_other_addrs = &other_addrs[0..init_limit];
            let mut other_app_pk = app_keypairs
                .iter()
                .map(|kp| kp.public())
                .collect::<Vec<_>>();
            other_app_pk.remove(idx);

            let user = User::new(
                app_keypair.clone(),
                transport_keypair.clone(),
                limited_other_addrs.to_vec(),
                addr.clone(),
                other_app_pk.to_vec(),
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
    /// If offset is 1234567890, then addresses will be /memory/1234567890 for n=1, for n=2
    /// /memory/1234567890 and /memory/1234567891 and so on.
    fn setup_keys_ids_addrs_of_n_users(n: usize, offset: u64) -> SetupInitialData {
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

        let multiaddresses = (offset..(offset + u64::try_from(n).unwrap()))
            .map(|idx| build_multiaddr!(Memory(idx)))
            .collect::<Vec<_>>();

        SetupInitialData {
            app_keypairs,
            transport_keypairs,
            multiaddresses,
        }
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
