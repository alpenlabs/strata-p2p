//! Helper functions for the P2P tests.

#[cfg(feature = "byos")]
use std::{pin::Pin, sync::Arc};
use std::{sync::Once, time::Duration};

use futures::future::join_all;
#[cfg(feature = "byos")]
use libp2p::identity::PublicKey;
use libp2p::{
    Multiaddr, PeerId, build_multiaddr, connection_limits::ConnectionLimits, identity::Keypair,
};
use rand::Rng;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::debug;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(feature = "byos")]
use crate::signer::ApplicationSigner;
#[cfg(feature = "gossipsub")]
use crate::swarm::handle::GossipHandle;
#[cfg(feature = "request-response")]
use crate::swarm::handle::ReqRespHandle;
use crate::swarm::{self, P2P, P2PConfig, handle::CommandHandle};
#[cfg(all(
    any(feature = "gossipsub", feature = "request-response"),
    not(feature = "byos")
))]
use crate::validator::{DefaultP2PValidator, Validator};

#[cfg(feature = "mem-conn-limits-abs")]
pub(crate) const SIXTEEN_GIBIBYTES: usize = 16 * 1024 * 1024 * 1024; // https://en.wikipedia.org/wiki/Byte#Multiple-byte_units

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
#[cfg(feature = "byos")]
#[derive(Debug, Clone)]
pub(crate) struct MockApplicationSigner {
    // Store the actual keypair that corresponds to the app public key
    app_keypair: Keypair,
}

#[cfg(feature = "byos")]
impl MockApplicationSigner {
    pub(crate) const fn new(app_keypair: Keypair) -> Self {
        Self { app_keypair }
    }
}

#[cfg(feature = "byos")]
impl ApplicationSigner for MockApplicationSigner {
    fn sign<'life0, 'life1, 'async_trait>(
        &'life0 self,
        message: &'life1 [u8],
    ) -> Pin<
        Box<
            dyn Future<Output = Result<[u8; 64], Box<dyn std::error::Error + Send + Sync>>>
                + Send
                + Sync
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            // Sign with the stored keypair
            let signature = self.app_keypair.sign(message)?;
            let sign_array: [u8; 64] = signature.try_into().unwrap();
            Ok(sign_array)
        })
    }
}

pub(crate) struct User {
    pub(crate) p2p: P2P,
    #[cfg(feature = "gossipsub")]
    pub(crate) gossip: GossipHandle,
    #[cfg(feature = "request-response")]
    pub(crate) reqresp: ReqRespHandle,
    pub(crate) command: CommandHandle,
    #[cfg(feature = "byos")]
    pub(crate) app_keypair: Keypair,
    #[cfg(any(feature = "gossipsub", feature = "byos", feature = "kad"))]
    pub(crate) transport_keypair: Keypair,
}

impl User {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        #[cfg(feature = "byos")] app_keypair: Keypair,
        transport_keypair: Keypair,
        connect_to: Vec<Multiaddr>,
        #[cfg(feature = "byos")] allowlist: Vec<PublicKey>,
        listening_addrs: Vec<Multiaddr>,
        cancel: CancellationToken,
        #[cfg(feature = "byos")] signer: Arc<dyn ApplicationSigner>,
        #[cfg(all(
            any(feature = "gossipsub", feature = "request-response"),
            not(feature = "byos")
        ))]
        validator: Box<dyn Validator>,
        conn_limits: ConnectionLimits,
        #[cfg(feature = "mem-conn-limits-abs")] max_allowed_ram_used: usize,
        #[cfg(feature = "mem-conn-limits-rel")] max_allowed_ram_used_percent: f64,
    ) -> anyhow::Result<Self> {
        debug!(
            ?listening_addrs,
            "Creating new user with listening addresses"
        );

        let config = P2PConfig {
            #[cfg(feature = "byos")]
            app_public_key: app_keypair.public(),
            transport_keypair: transport_keypair.clone(),
            idle_connection_timeout: Duration::from_secs(30),
            max_retries: None,
            dial_timeout: None,
            general_timeout: None,
            connection_check_interval: None,
            listening_addrs,
            connect_to,
            protocol_name: None,
            #[cfg(feature = "gossipsub")]
            gossipsub_topic: None,
            #[cfg(feature = "gossipsub")]
            gossipsub_max_transmit_size: None,
            envelope_max_age: None,
            max_clock_skew: None,
            #[cfg(feature = "request-response")]
            channel_timeout: None,
            #[cfg(feature = "gossipsub")]
            gossipsub_score_params: None,
            #[cfg(feature = "gossipsub")]
            gossipsub_score_thresholds: None,
            #[cfg(feature = "gossipsub")]
            gossip_event_buffer_size: None,
            command_buffer_size: None,
            commands_event_buffer_size: None,
            handle_default_timeout: None,
            #[cfg(feature = "request-response")]
            req_resp_event_buffer_size: None,
            #[cfg(feature = "request-response")]
            req_resp_command_buffer_size: None,
            #[cfg(feature = "request-response")]
            request_max_bytes: None,
            #[cfg(feature = "request-response")]
            response_max_bytes: None,
            #[cfg(feature = "gossipsub")]
            gossip_command_buffer_size: None,
            #[cfg(feature = "kad")]
            kad_protocol_name: None, // this will take default one.
            conn_limits,
            #[cfg(feature = "mem-conn-limits-abs")]
            max_allowed_ram_used,
            #[cfg(feature = "mem-conn-limits-rel")]
            max_allowed_ram_used_percent,
            #[cfg(feature = "kad")]
            kad_record_ttl: None,
            #[cfg(feature = "kad")]
            kad_timer_putrecorderror: None,
        };

        // Determine transport type based on the first listening address
        let use_inmemory = config
            .listening_addrs
            .first()
            .map(|addr| addr.to_string().starts_with("/memory/"))
            .unwrap_or(false);

        let swarm = if use_inmemory {
            swarm::with_inmemory_transport(
                &config,
                #[cfg(feature = "byos")]
                signer.clone(),
            )?
        } else {
            swarm::with_default_transport(
                &config,
                #[cfg(feature = "byos")]
                signer.clone(),
            )?
        };

        #[cfg(feature = "request-response")]
        let (p2p, reqresp) = P2P::from_config(
            config,
            cancel,
            swarm,
            #[cfg(feature = "byos")]
            allowlist,
            #[cfg(feature = "gossipsub")]
            None,
            #[cfg(feature = "byos")]
            signer.clone(),
            #[cfg(not(feature = "byos"))]
            Some(validator),
        )?;
        #[cfg(not(feature = "request-response"))]
        let p2p = P2P::from_config(
            config,
            cancel,
            swarm,
            #[cfg(feature = "byos")]
            allowlist,
            #[cfg(feature = "gossipsub")]
            None,
            #[cfg(all(feature = "byos", any(feature = "gossipsub", feature = "kad")))]
            signer,
            #[cfg(all(feature = "gossipsub", not(feature = "byos")))]
            Some(validator),
        )?;
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
            #[cfg(feature = "byos")]
            app_keypair,
            #[cfg(any(feature = "gossipsub", feature = "byos", feature = "kad"))]
            transport_keypair,
        })
    }
}

/// Auxiliary structure to control users from outside.
#[cfg_attr(not(feature = "gossipsub"), allow(dead_code))]
pub(crate) struct UserHandle {
    pub(crate) peer_id: PeerId,
    #[cfg(feature = "gossipsub")]
    pub(crate) gossip: GossipHandle,
    #[cfg(feature = "request-response")]
    pub(crate) reqresp: ReqRespHandle,
    pub(crate) command: CommandHandle,
    #[cfg(feature = "byos")]
    pub(crate) app_keypair: Keypair,
}

pub(crate) struct Setup {
    pub(crate) cancel: CancellationToken,
    pub(crate) user_handles: Vec<UserHandle>,
    pub(crate) tasks: TaskTracker,
}

pub(crate) struct SetupInitialData {
    #[cfg(feature = "byos")]
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
            #[cfg(feature = "byos")]
            app_keypairs,
            transport_keypairs,
            peer_ids,
            multiaddresses,
        } = Self::setup_keys_ids_addrs_of_n_users(n);

        let cancel = CancellationToken::new();
        let mut users = Vec::new();

        #[cfg(feature = "byos")]
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
                #[cfg(feature = "byos")]
                Arc::new(MockApplicationSigner {
                    app_keypair: app_keypair.clone(),
                }),
                ConnectionLimits::default().with_max_established(Some(u32::MAX)),
                #[cfg(feature = "mem-conn-limits-abs")]
                SIXTEEN_GIBIBYTES,
                #[cfg(feature = "mem-conn-limits-rel")]
                1.0, // 100 %
            )?;

            users.push(user);
        }

        #[cfg(not(feature = "byos"))]
        for (idx, (transport_keypair, addr)) in
            transport_keypairs.iter().zip(&multiaddresses).enumerate()
        {
            let mut other_addrs = multiaddresses.clone();
            other_addrs.remove(idx);
            let mut other_peerids = peer_ids.clone();
            other_peerids.remove(idx);

            let user = User::new(
                transport_keypair.clone(),
                other_addrs,
                vec![addr.clone()],
                cancel.child_token(),
                #[cfg(any(feature = "gossipsub", feature = "request-response"))]
                Box::new(DefaultP2PValidator),
                ConnectionLimits::default().with_max_established(Some(u32::MAX)),
                #[cfg(feature = "mem-conn-limits-abs")]
                SIXTEEN_GIBIBYTES,
                #[cfg(feature = "mem-conn-limits-rel")]
                1.0, // 100 %
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

    /// Spawn `n` users that are connected "all-to-all" with a new user's public key
    /// pre-included in their allowlist.
    #[cfg(all(feature = "byos", any(feature = "gossipsub", feature = "kad")))]
    pub(crate) async fn all_to_all_with_new_user_allowlist(
        n: usize,
        new_user_app_public_key: &PublicKey,
    ) -> anyhow::Result<Self> {
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

            #[cfg(feature = "byos")]
            let mut allowlist = app_keypairs
                .iter()
                .map(|kp| kp.public())
                .collect::<Vec<_>>();
            #[cfg(feature = "byos")]
            allowlist.remove(idx);
            #[cfg(feature = "byos")]
            allowlist.push(new_user_app_public_key.clone());

            let user = User::new(
                app_keypair.clone(),
                transport_keypair.clone(),
                other_addrs,
                #[cfg(feature = "byos")]
                allowlist,
                vec![addr.clone()],
                cancel.child_token(),
                #[cfg(feature = "byos")]
                Arc::new(MockApplicationSigner {
                    app_keypair: app_keypair.clone(),
                }),
                #[cfg(not(feature = "byos"))]
                Box::new(DefaultP2PValidator),
                ConnectionLimits::default().with_max_established(Some(u32::MAX)),
                #[cfg(feature = "mem-conn-limits-abs")]
                SIXTEEN_GIBIBYTES,
                #[cfg(feature = "mem-conn-limits-rel")]
                1.0, // 100 %
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

    /// Spawn `n` users that are connected "all-to-all" with a custom validator.
    #[cfg(all(
        any(feature = "gossipsub", feature = "request-response"),
        not(feature = "byos")
    ))]
    pub(crate) async fn all_to_all_with_custom_validator<V: Validator + Clone>(
        n: usize,
        validator: V,
    ) -> anyhow::Result<Self> {
        let SetupInitialData {
            transport_keypairs,
            peer_ids,
            multiaddresses,
        } = Self::setup_keys_ids_addrs_of_n_users(n);

        let cancel = CancellationToken::new();
        let mut users = Vec::new();

        #[cfg(not(feature = "byos"))]
        for (idx, (transport_keypair, addr)) in
            transport_keypairs.iter().zip(&multiaddresses).enumerate()
        {
            let mut other_addrs = multiaddresses.clone();
            other_addrs.remove(idx);
            let mut other_peerids = peer_ids.clone();
            other_peerids.remove(idx);

            let user = User::new(
                transport_keypair.clone(),
                other_addrs,
                vec![addr.clone()],
                cancel.child_token(),
                Box::new(validator.clone()),
                ConnectionLimits::default().with_max_established(Some(u32::MAX)),
                #[cfg(feature = "mem-conn-limits-abs")]
                SIXTEEN_GIBIBYTES,
                #[cfg(feature = "mem-conn-limits-rel")]
                1.0, // 100 %
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
        #[cfg(feature = "byos")]
        let app_keypairs = (0..n)
            .map(|_| Keypair::generate_ed25519())
            .collect::<Vec<_>>();

        let transport_keypairs = (0..n)
            .map(|_| Keypair::generate_ed25519())
            .collect::<Vec<_>>();

        #[cfg(feature = "byos")]
        debug!(
            len_app_keypairs = %app_keypairs.len(),
        );

        debug!(
            len_transport_keypairs= %transport_keypairs.len()
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
            #[cfg(feature = "byos")]
            app_keypairs,
            transport_keypairs,
            peer_ids,
            multiaddresses,
        }
    }

    /// Waits for all users to establish connections and subscriptions, then spawns their listen
    /// tasks. Returns user handles for communication and a task tracker for managing the
    /// spawned tasks.
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
                #[cfg(feature = "byos")]
                app_keypair: user.app_keypair,
            });
        }

        tasks.close();
        (levers, tasks)
    }
}
