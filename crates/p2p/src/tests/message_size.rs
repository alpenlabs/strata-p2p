// //! Test the size of messages in the p2p gossipsub protocol using the [`DepositSetup`] message.
//
// use anyhow::bail;
// use bitcoin::{
//     hashes::{sha256, Hash},
//     Txid, XOnlyPublicKey,
// };
// use strata_p2p_types::{
//     Groth16PublicKeys, Scope, Wots128PublicKey, Wots256PublicKey, WotsPublicKeys,
// };
// use strata_p2p_wire::p2p::v1::{GossipsubMsg, UnsignedGossipsubMsg};
// use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
//
// use crate::{
//     commands::{Command, UnsignedPublishMessage},
//     events::Event,
//     tests::common::Setup,
// };
//
// const N_PUBLIC_INPUTS: usize = 1;
// const N_FIELD_ELEMENTS: usize = 14;
// const N_HASHES: usize = 363;
//
// fn real_deposit_setup() -> anyhow::Result<UnsignedPublishMessage> {
//     let scope = Scope::hash(b"scope");
//     let wots_pks = WotsPublicKeys {
//         withdrawal_fulfillment: Wots256PublicKey::from_flattened_bytes(&[1u8; 68 * 20]),
//         groth16: Groth16PublicKeys::new(
//             vec![Wots256PublicKey::from_flattened_bytes(&[2u8; 68 * 20]); N_PUBLIC_INPUTS],
//             vec![Wots256PublicKey::from_flattened_bytes(&[3u8; 68 * 20]); N_FIELD_ELEMENTS],
//             vec![Wots128PublicKey::from_flattened_bytes(&[4u8; 36 * 20]); N_HASHES],
//         ),
//     };
//     Ok(UnsignedPublishMessage::DepositSetup {
//         scope,
//         index: 1,
//         hash: sha256::Hash::hash(b"hash"),
//         funding_txid: Txid::all_zeros(),
//         funding_vout: 0,
//         operator_pk: XOnlyPublicKey::from_slice(&[2u8; 32]).unwrap(),
//         wots_pks,
//     })
// }
//
// /// Tests the gossip protocol in an all to all connected network with a single ID.
// #[tokio::test(flavor = "multi_thread", worker_threads = 3)]
// async fn all_to_all_message_size() -> anyhow::Result<()> {
//     const OPERATORS_NUM: usize = 2;
//
//     tracing_subscriber::registry()
//         .with(fmt::layer())
//         .with(EnvFilter::from_default_env())
//         .init();
//
//     let Setup { mut operators, .. } = Setup::all_to_all(OPERATORS_NUM).await?;
//
//     let unsigned = real_deposit_setup()?;
//     let command: Command = unsigned.sign_secp256k1(&operators[0].kp).into();
//     operators[0].handle.send_command(command).await;
//
//     let event = operators[1].handle.next_event().await?;
//
//     if !matches!(
//         event,
//         Event::ReceivedMessage(GossipsubMsg {
//             unsigned: UnsignedGossipsubMsg::DepositSetup { .. },
//             ..
//         })
//     ) {
//         bail!("Got event other than the sent one - {:?}", event);
//     }
//
//     let (signature, key, message) = match event {
//         Event::ReceivedMessage(gossipsub_msg) => (
//             gossipsub_msg.signature.clone(),
//             gossipsub_msg.key.clone(),
//             gossipsub_msg.content(),
//         ),
//         _ => bail!("Got event other than the sent one - {:?}", event),
//     };
//
//     // Verify signature against key
//
//     assert!(key.verify(&message, &signature));
//
//     assert!(operators[1].handle.events_is_empty());
//
//     Ok(())
// }
