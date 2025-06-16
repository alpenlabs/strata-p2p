// //! Tests for the `is_connected` method.
//
// use crate::tests::common::Setup;
//
// #[tokio::test]
// async fn test_is_connected() -> anyhow::Result<()> {
//     // Set up two connected operators
//     let Setup {
//         operators,
//         cancel,
//         tasks,
//     } = Setup::all_to_all(2).await?;
//
//     // Verify operator 0 is connected to operator 1
//     let is_connected = operators[0].handle.is_connected(operators[1].peer_id).await;
//     assert!(is_connected);
//
//     // Also test the get_connected_peers API
//     let connected_peers = operators[0].handle.get_connected_peers().await;
//     assert!(connected_peers.contains(&operators[1].peer_id));
//
//     // Cleanup
//     cancel.cancel();
//     tasks.wait().await;
//
//     Ok(())
// }
