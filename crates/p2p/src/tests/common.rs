//! Helper functions for the P2P tests.

use std::time::Duration;

use futures::future::join_all;
<<<<<<< HEAD
use libp2p::{build_multiaddr, identity::Keypair, Multiaddr, PeerId};
use rand::Rng;
=======
use libp2p::{
    build_multiaddr,
    identity::{secp256k1::Keypair as SecpKeypair, Keypair},
    Multiaddr, PeerId,
};
use strata_p2p_types::{P2POperatorPubKey, Scope, SessionId};
>>>>>>> 3536945 (run single gossipsub test)
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::debug;

<<<<<<< HEAD
#[cfg(feature = "request-response")]
use crate::swarm::handle::ReqRespHandle;
use crate::swarm::{
    self,
    handle::{CommandHandle, GossipHandle},
    P2PConfig, P2P,
=======
use crate::{
    commands::{Command},
    events::Event,
    swarm::{self, handle::P2PHandle, P2PConfig, P2P},
>>>>>>> 3536945 (run single gossipsub test)
};

pub(crate) struct User {
    pub(crate) p2p: P2P,
    pub(crate) gossip: GossipHandle,
    #[cfg(feature = "request-response")]
    pub(crate) reqresp: ReqRespHandle,
    pub(crate) command: CommandHandle,
    pub(crate) kp: Keypair,
}

impl User {
    pub(crate) fn new(
        keypair: Keypair,
        allowlist: Vec<PeerId>,
        connect_to: Vec<Multiaddr>,
        local_addr: Multiaddr,
        cancel: CancellationToken,
    ) -> anyhow::Result<Self> {
        debug!(%local_addr, "Creating new user with local address");

        let config = P2PConfig {
            keypair: keypair.clone(),
            idle_connection_timeout: Duration::from_secs(30),
            max_retries: None,
            dial_timeout: None,
            general_timeout: None,
            connection_check_interval: None,
            listening_addr: local_addr,
            allowlist,
            connect_to,
            channel_timeout: None,
        };

        let swarm = swarm::with_inmemory_transport(&config)?;
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
            kp: keypair,
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
    pub(crate) async fn all_to_all(n: usize) -> anyhow::Result<Self> {
        let (keypairs, peer_ids, multiaddresses) = Self::setup_keys_ids_addrs_of_n_users(n);

        let cancel = CancellationToken::new();
        let mut users = Vec::new();

        for (idx, (keypair, addr)) in keypairs.iter().zip(&multiaddresses).enumerate() {
            let mut other_addrs = multiaddresses.clone();
            other_addrs.remove(idx);
            let mut other_peerids = peer_ids.clone();
            other_peerids.remove(idx);

            let user = User::new(
                keypair.clone(),
                other_peerids,
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
<<<<<<< HEAD
            user_handles: users,
        })
    }
=======
            operators,
        })
    }

    // /// Spawn `n` operators that are connected "all-to-all" with handles to them, task tracker
    // /// to stop control async tasks they are spawned in with an extra signers allowlist.
    // pub(crate) async fn with_extra_signers(
    //     number: usize,
    //     extra_signers: Vec<P2POperatorPubKey>,
    // ) -> anyhow::Result<Self> {
    //     let (keypairs, peer_ids, multiaddresses) =
    //         Self::setup_keys_ids_addrs_of_n_operators(number);
    //
    //     let cancel = CancellationToken::new();
    //     let mut operators = Vec::new();
    //     let mut signers_allowlist: Vec<P2POperatorPubKey> = keypairs
    //         .clone()
    //         .into_iter()
    //         .map(|kp| kp.public().clone().into())
    //         .collect();
    //
    //     // Add the extra signers to the allowlist
    //     signers_allowlist.extend(extra_signers);
    //
    //     for (idx, (keypair, addr)) in keypairs.iter().zip(&multiaddresses).enumerate() {
    //         let mut other_addrs = multiaddresses.clone();
    //         other_addrs.remove(idx);
    //         let mut other_peerids = peer_ids.clone();
    //         other_peerids.remove(idx);
    //
    //         let operator = Operator::new(
    //             keypair.clone(),
    //             other_peerids,
    //             other_addrs,
    //             addr.clone(),
    //             cancel.child_token(),
    //             signers_allowlist.clone(),
    //         )?;
    //
    //         operators.push(operator);
    //     }
    //
    //     let (operators, tasks) = Self::start_operators(operators).await;
    //
    //     Ok(Self {
    //         cancel,
    //         tasks,
    //         operators,
    //     })
    // }
>>>>>>> 3536945 (run single gossipsub test)

    /// Create `n` random keypairs, peer ids from them and sequential in-memory
    /// addresses.
    fn setup_keys_ids_addrs_of_n_users(
        n: usize,
    ) -> (Vec<Keypair>, Vec<PeerId>, Vec<libp2p::Multiaddr>) {
        let keypairs = (0..n)
            .map(|_| Keypair::generate_ed25519())
            .collect::<Vec<_>>();

        debug!(len = %keypairs.len(), "Generated keypairs for test setup");

        let peer_ids = keypairs
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
            ..(multiaddr_base + u64::try_from(keypairs.len()).unwrap()))
            .map(|idx| build_multiaddr!(Memory(idx)))
            .collect::<Vec<_>>();
        (keypairs, peer_ids, multiaddresses)
    }

    /// Wait until all users established connections with other users,
    /// and then spawn [`P2P::listen`]s in separate tasks using [`TaskTracker`].
    async fn start_users(mut users: Vec<User>) -> (Vec<UserHandle>, TaskTracker) {
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
                kp: user.kp,
            });
        }

        tasks.close();
        (levers, tasks)
    }
}
<<<<<<< HEAD
=======
//
// pub(crate) fn mock_stake_chain_info(kp: &SecpKeypair, stake_chain_id: StakeChainId) -> Command {
//     let kind = UnsignedPublishMessage::StakeChainExchange {
//         stake_chain_id,
//         // some random point
//         operator_pk: XOnlyPublicKey::from_slice(&[2u8; 32]).unwrap(),
//         pre_stake_txid: Txid::all_zeros(),
//         pre_stake_vout: 0,
//     };
//     kind.sign_secp256k1(kp).into()
// }
//
// pub(crate) fn mock_deposit_setup(kp: &SecpKeypair, scope: Scope) -> Command {
//     let mock_bytes = [0u8; 1_360 + 362_960];
//     let unsigned = UnsignedPublishMessage::DepositSetup {
//         scope,
//         index: 0,
//         hash: sha256::Hash::const_hash(b"hash me!"),
//         funding_txid: Txid::all_zeros(),
//         funding_vout: 0,
//         operator_pk: XOnlyPublicKey::from_slice(&[2u8; 32]).unwrap(),
//         wots_pks: WotsPublicKeys::from_flattened_bytes(&mock_bytes),
//     };
//     unsigned.sign_secp256k1(kp).into()
// }
//
// pub(crate) fn mock_deposit_nonces(kp: &SecpKeypair, session_id: SessionId) -> Command {
//     let unsigned = UnsignedPublishMessage::Musig2NoncesExchange {
//         session_id,
//         pub_nonces: (0..5).map(|_| generate_pubnonce()).collect(),
//     };
//     unsigned.sign_secp256k1(kp).into()
// }
//
// pub(crate) fn mock_deposit_sigs(kp: &SecpKeypair, session_id: SessionId) -> Command {
//     let unsigned = UnsignedPublishMessage::Musig2SignaturesExchange {
//         session_id,
//         partial_sigs: (0..5).map(|_| generate_partial_signature()).collect(),
//     };
//     unsigned.sign_secp256k1(kp).into()
// }
//
// pub(crate) async fn exchange_stake_chain_info(
//     operators: &mut [OperatorHandle],
//     operators_num: usize,
//     stake_chain_id: StakeChainId,
// ) -> anyhow::Result<()> {
//     for operator in operators.iter() {
//         operator
//             .handle
//             .send_command(mock_stake_chain_info(&operator.kp, stake_chain_id))
//             .await;
//     }
//     for operator in operators.iter_mut() {
//         // received stake chain info from other n-1 operators
//         for _ in 0..operators_num - 1 {
//             let event = operator.handle.next_event().await?;
//
//             if !matches!(
//                 event,
//                 Event::ReceivedMessage(GossipsubMsg {
//                     unsigned: UnsignedGossipsubMsg::StakeChainExchange { .. },
//                     ..
//                 })
//             ) {
//                 bail!("Got event other than 'stake_chain_info' - {:?}", event);
//             }
//         }
//
//         assert!(operator.handle.events_is_empty());
//     }
//
//     Ok(())
// }
//
// pub(crate) async fn exchange_deposit_setup(
//     operators: &mut [OperatorHandle],
//     operators_num: usize,
//     scope: Scope,
// ) -> anyhow::Result<()> {
//     for operator in operators.iter() {
//         operator
//             .handle
//             .send_command(mock_deposit_setup(&operator.kp, scope))
//             .await;
//     }
//     for operator in operators.iter_mut() {
//         for _ in 0..operators_num - 1 {
//             let event = operator.handle.next_event().await.unwrap();
//             if !matches!(
//                 event,
//                 Event::ReceivedMessage(GossipsubMsg {
//                     unsigned: UnsignedGossipsubMsg::DepositSetup { .. },
//                     ..
//                 })
//             ) {
//                 bail!("Got event other than 'deposit_setup' - {:?}", event);
//             }
//             info!(to=%operator.peer_id, "Got deposit setup");
//         }
//         assert!(operator.handle.events_is_empty());
//     }
//     Ok(())
// }
//
// pub(crate) async fn exchange_deposit_nonces(
//     operators: &mut [OperatorHandle],
//     operators_num: usize,
//     session_id: SessionId,
// ) -> anyhow::Result<()> {
//     for operator in operators.iter() {
//         operator
//             .handle
//             .send_command(mock_deposit_nonces(&operator.kp, session_id))
//             .await;
//     }
//     for operator in operators.iter_mut() {
//         for _ in 0..operators_num - 1 {
//             let event = operator.handle.next_event().await.unwrap();
//             if !matches!(
//                 event,
//                 Event::ReceivedMessage(GossipsubMsg {
//                     unsigned: UnsignedGossipsubMsg::Musig2NoncesExchange { .. },
//                     ..
//                 })
//             ) {
//                 bail!("Got event other than 'deposit_nonces' - {:?}", event);
//             }
//             info!(to=%operator.peer_id, "Got deposit setup");
//         }
//         assert!(operator.handle.events_is_empty());
//     }
//     Ok(())
// }
//
// pub(crate) async fn exchange_deposit_sigs(
//     operators: &mut [OperatorHandle],
//     operators_num: usize,
//     session_id: SessionId,
// ) -> anyhow::Result<()> {
//     for operator in operators.iter() {
//         operator
//             .handle
//             .send_command(mock_deposit_sigs(&operator.kp, session_id))
//             .await;
//     }
//
//     for operator in operators.iter_mut() {
//         for _ in 0..operators_num - 1 {
//             let event = operator.handle.next_event().await.unwrap();
//             if !matches!(
//                 event,
//                 Event::ReceivedMessage(GossipsubMsg {
//                     unsigned: UnsignedGossipsubMsg::Musig2SignaturesExchange { .. },
//                     ..
//                 })
//             ) {
//                 bail!("Got event other than 'deposit_sigs' - {:?}", event);
//             }
//             info!(to=%operator.peer_id, "Got deposit sigs");
//         }
//         assert!(operator.handle.events_is_empty());
//     }
//
//     Ok(())
// }
//
// /// Size of the nonce seed in bytes.
// const NONCE_SEED_SIZE: usize = 32;
//
// /// Generates a mock public nonce.
// pub(crate) fn generate_pubnonce() -> PubNonce {
//     let sec_nonce = generate_secnonce();
//
//     sec_nonce.public_nonce()
// }
//
// /// Generates a mock secret nonce.
// pub(crate) fn generate_secnonce() -> SecNonce {
//     let mut nonce_seed_bytes = [0u8; NONCE_SEED_SIZE];
//     OsRng.fill(&mut nonce_seed_bytes);
//     let nonce_seed = NonceSeed::from(nonce_seed_bytes);
//
//     SecNonce::build(nonce_seed).build()
// }
//
// /// Generates a mock partial signature.
// pub(crate) fn generate_partial_signature() -> PartialSignature {
//     let secret_key = SecretKey::new(&mut OsRng);
//
//     PartialSignature::from_slice(secret_key.as_ref())
//         .expect("should be able to generate arbitrary partial signature")
// }
>>>>>>> 3536945 (run single gossipsub test)
