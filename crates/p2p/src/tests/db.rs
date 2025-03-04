//! DB tests.

use strata_p2p_db::{sled::AsyncDB, RepositoryExt};
use strata_p2p_types::{OperatorPubKey, Scope, SessionId, StakeChainId};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use super::common::{
    exchange_deposit_nonces, exchange_deposit_setup, exchange_deposit_sigs,
    exchange_stake_chain_info, Setup,
};
use crate::commands::CleanStorageCommand;

#[tokio::test]
async fn operator_cleans_entries_correctly_at_command() -> anyhow::Result<()> {
    const OPERATORS_NUM: usize = 2;

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let Setup {
        mut operators,
        cancel,
        tasks,
    } = Setup::all_to_all(OPERATORS_NUM).await?;

    let stake_chain_id = StakeChainId::hash(b"stake_chain_id");
    let scope = Scope::hash(b"scope");
    let session_id = SessionId::hash(b"session_id");

    exchange_stake_chain_info(&mut operators, OPERATORS_NUM, stake_chain_id).await?;
    exchange_deposit_setup(&mut operators, OPERATORS_NUM, scope).await?;
    exchange_deposit_nonces(&mut operators, OPERATORS_NUM, session_id).await?;
    exchange_deposit_sigs(&mut operators, OPERATORS_NUM, session_id).await?;

    let other_operator_pubkey = OperatorPubKey::from(operators[0].kp.public().to_bytes().to_vec());
    let last_operator = &mut operators[1];
    last_operator
        .handle
        .send_command(CleanStorageCommand::new(
            vec![scope],
            vec![session_id],
            vec![other_operator_pubkey.clone()],
        ))
        .await;

    cancel.cancel();
    tasks.wait().await;

    // Check that storage is empty after that.
    let setup_entry = <AsyncDB as RepositoryExt>::get_deposit_setup(
        &last_operator.db,
        &other_operator_pubkey,
        scope,
    )
    .await?;
    assert!(setup_entry.is_none());

    let nonces_entry = <AsyncDB as RepositoryExt>::get_pub_nonces(
        &last_operator.db,
        &other_operator_pubkey,
        session_id,
    )
    .await?;
    assert!(nonces_entry.is_none());

    let sigs_entry = <AsyncDB as RepositoryExt>::get_partial_signatures(
        &last_operator.db,
        &other_operator_pubkey,
        session_id,
    )
    .await?;
    assert!(sigs_entry.is_none());

    Ok(())
}
