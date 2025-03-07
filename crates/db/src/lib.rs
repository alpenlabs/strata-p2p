//! Serialized data storage for the P2P protocol.
#![expect(incomplete_features)] // the generic_const_exprs feature is incomplete
#![feature(generic_const_exprs)] // but necessary for using const generic bounds in

use async_trait::async_trait;
use bitcoin::{hashes::sha256, Txid, XOnlyPublicKey};
use libp2p_identity::PeerId;
use musig2::{PartialSignature, PubNonce};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use strata_p2p_types::{P2POperatorPubKey, Scope, SessionId, StakeChainId, WotsPublicKeys};
use thiserror::Error;

mod prost_serde;
pub mod sled;

pub type DBResult<T> = Result<T, RepositoryError>;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("Storage error: {0}")]
    Storage(#[from] Box<dyn std::error::Error + Send + Sync>),
    #[error("Invalid data error: {0}")]
    InvalidData(Box<dyn std::error::Error + Send + Sync>),
}

impl From<serde_json::Error> for RepositoryError {
    fn from(err: serde_json::Error) -> Self {
        Self::InvalidData(Box::new(err))
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthenticatedEntry<T> {
    pub entry: T,
    pub signature: Vec<u8>,
    pub key: P2POperatorPubKey,
}

/// A [`Vec`] of [`PartialSignature`]s.
pub type PartialSignaturesEntry = AuthenticatedEntry<Vec<PartialSignature>>;

/// A [`Vec`] of [`PubNonce`]s.
pub type NoncesEntry = AuthenticatedEntry<Vec<PubNonce>>;

/// A tuple of:
///
/// 1. [`Txid`] of the pre-stake transaction.
/// 2. vout index of the pre-stake transaction.
pub type StakeChainEntry = AuthenticatedEntry<(Txid, u32)>;

/// Basic functionality to get, set, and delete values from a Database.
#[async_trait]
pub trait Repository: Send + Sync + 'static {
    async fn get_raw(&self, key: String) -> DBResult<Option<Vec<u8>>>;
    async fn set_raw(&self, key: String, value: Vec<u8>) -> DBResult<()>;

    /// Delete all values with provided keys.
    async fn delete_raw(&self, keys: Vec<String>) -> DBResult<()>;

    /// Set new value if it wasn't there before. Default implementation is
    /// just `get`+`set`, but some databases may have more optimized
    /// implementation in one go.
    ///
    /// Returns `true` if `value` wasn't there before.
    async fn set_raw_if_not_exists(&self, key: String, value: Vec<u8>) -> DBResult<bool> {
        if self.get_raw(key.clone()).await?.is_some() {
            return Ok(false);
        }
        self.set_raw(key, value).await?;

        Ok(true)
    }

    async fn set_if_not_exists<T>(&self, key: String, value: T) -> DBResult<bool>
    where
        T: Serialize + Send + Sync + 'static,
    {
        let mut buf = Vec::new();
        serde_json::to_writer(&mut buf, &value)?;

        self.set_raw_if_not_exists(key, buf).await
    }

    async fn get<T: DeserializeOwned>(&self, key: String) -> DBResult<Option<T>> {
        let bytes = self.get_raw(key).await?;
        let Some(bytes) = bytes else {
            return Ok(None);
        };

        let entry: T = serde_json::from_reader(bytes.as_slice())?;

        Ok(Some(entry))
    }

    async fn set<T>(&self, key: String, value: T) -> DBResult<()>
    where
        T: Serialize + Send + Sync + 'static,
    {
        let mut buf = Vec::new();
        serde_json::to_writer(&mut buf, &value)?;

        self.set_raw(key, buf).await?;

        Ok(())
    }
}

/// Additional functionality that extends [`Repository`] to the specific needs of Deposit Setups,
/// Musig2 (public) nonces and (partial) signatures; and peer id storage.
#[async_trait]
pub trait RepositoryExt: Repository {
    /// Gets (partial) signatures for a given a P2P [`P2POperatorPubKey`] and [`SessionId`].
    async fn get_partial_signatures(
        &self,
        operator_pk: &P2POperatorPubKey,
        session_id: SessionId,
    ) -> DBResult<Option<PartialSignaturesEntry>> {
        let key = format!("sigs-{operator_pk}_{session_id}");
        self.get(key).await
    }

    /// Sets partial signatures for a given [`SessionId`] if they weren't there before.
    async fn set_partial_signatures_if_not_exists(
        &self,
        session_id: SessionId,
        entry: PartialSignaturesEntry,
    ) -> DBResult<bool> {
        let key = format!("sigs-{}_{session_id}", entry.key);
        self.set_if_not_exists(key, entry).await
    }

    /// Deletes multiple entries of partial signatures from storage by pairs of
    /// P2P [`P2POperatorPubKey`]s and [`SessionId`]s.
    async fn delete_partial_signatures(
        &self,
        keys: &[(&P2POperatorPubKey, &SessionId)],
    ) -> DBResult<()> {
        let keys = keys
            .iter()
            .map(|(key, id)| format!("sigs-{key}_{id}"))
            .collect::<Vec<_>>();
        self.delete_raw(keys).await
    }

    /// Gets (public) nonces for a given P2P [`P2POperatorPubKey`] and [`SessionId`].
    async fn get_pub_nonces(
        &self,
        operator_pk: &P2POperatorPubKey,
        session_id: SessionId,
    ) -> DBResult<Option<NoncesEntry>> {
        let key = format!("nonces-{operator_pk}_{session_id}");
        self.get(key).await
    }

    /// Sets public nonces for a given [`SessionId`] if they weren't there before.
    async fn set_pub_nonces_if_not_exist(
        &self,
        session_id: SessionId,
        entry: NoncesEntry,
    ) -> DBResult<bool> {
        let key = format!("nonces-{}_{session_id}", entry.key);
        self.set_if_not_exists(key, entry).await
    }

    /// Delete multiple entries of (public) nonces from storage by pairs of
    /// P2P [`P2POperatorPubKey`]s and [`SessionId`]s.
    async fn delete_pub_nonces(&self, keys: &[(&P2POperatorPubKey, &SessionId)]) -> DBResult<()> {
        let keys = keys
            .iter()
            .map(|(key, id)| format!("nonces-{key}_{id}"))
            .collect::<Vec<_>>();
        self.delete_raw(keys).await
    }

    /// Gets deposit setup for a given P2P [`P2POperatorPubKey`] and [`Scope`].
    ///
    /// This is primarily used for the WOTS PKs.
    async fn get_deposit_setup(
        &self,
        operator_pk: &P2POperatorPubKey,
        scope: Scope,
    ) -> DBResult<Option<DepositSetupEntry>> {
        let key = format!("setup-{operator_pk}_{scope}");
        self.get(key).await
    }

    /// Sets deposit setup for a given [`Scope`] if it wasn't there before.
    ///
    /// This is primarily used for the WOTS PKs.
    async fn set_deposit_setup_if_not_exists(
        &self,
        scope: Scope,
        setup: DepositSetupEntry,
    ) -> DBResult<bool> {
        let key = format!("setup-{}_{scope}", setup.key);
        self.set_if_not_exists(key, setup).await
    }

    /// Delete multiple entries of deposit setups from storage by pairs of
    /// P2P [`P2POperatorPubKey`]s and [`Scope`]s.
    ///
    /// This is primarily used for the WOTS PKs.
    async fn delete_deposit_setups(&self, keys: &[(&P2POperatorPubKey, &Scope)]) -> DBResult<()> {
        let keys = keys
            .iter()
            .map(|(key, scope)| format!("setup-{key}_{scope}"))
            .collect::<Vec<_>>();
        self.delete_raw(keys).await
    }

    /// Gets stake chain info for a given P2P [`P2POperatorPubKey`] and [`StakeChainId`].
    async fn get_stake_chain_info(
        &self,
        operator_pk: &P2POperatorPubKey,
        stake_chain_id: &StakeChainId,
    ) -> DBResult<Option<StakeChainEntry>> {
        let key = format!("stake-chain-{operator_pk}_{stake_chain_id}");
        self.get(key).await
    }

    /// Sets stake chain info for a given [`SessionId`] if they weren't there before.
    async fn set_stake_chain_info_if_not_exists(
        &self,
        stake_chain_id: StakeChainId,
        info: StakeChainEntry,
    ) -> DBResult<bool> {
        let key = format!("stake-chain-{}_{stake_chain_id}", info.key);
        self.set_if_not_exists(key, info).await
    }

    /// Deletes multiple entries of stake chains from storage by pairs of
    /// P2P [`P2POperatorPubKey`]s and [`StakeChainId`]s.
    async fn delete_stake_chains(
        &self,
        keys: &[(&P2POperatorPubKey, &StakeChainId)],
    ) -> DBResult<()> {
        let keys = keys
            .iter()
            .map(|(key, id)| format!("stake-chain-{key}_{id}"))
            .collect::<Vec<_>>();
        self.delete_raw(keys).await
    }

    /* P2P stores mapping of Musig2 exchange signers (operators) to node peer
    id that publishes messages. These methods store and retrieve this
    mapping:  */

    /// Get peer id of node, that distributed message signed by an P2P [`P2POperatorPubKey`].
    async fn get_peer_by_signer_pubkey(
        &self,
        operator_pk: &P2POperatorPubKey,
    ) -> DBResult<Option<PeerId>> {
        let key = format!("signers=>peerid-{}", operator_pk);
        self.get(key).await
    }

    /// Store peer id of node, that distributed message signed by this
    /// operator.
    async fn set_peer_for_signer_pubkey(
        &self,
        operator_pk: &P2POperatorPubKey,
        peer_id: PeerId,
    ) -> DBResult<()> {
        let key = format!("signers=>peerid-{}", operator_pk);
        self.set(key, peer_id).await
    }
}

impl<T> RepositoryExt for T where T: Repository {}

/// Information that is gossiped or requested by other nodes when a deposit occurs.
#[derive(Debug, Serialize, Deserialize)]
pub struct DepositSetupEntry {
    /// [`sha256::Hash`] hash of the stake transaction that the preimage is revealed when advancing
    /// the stake.
    pub hash: sha256::Hash,

    /// Funding transaction ID.
    ///
    /// Used to cover the dust outputs in the transaction graph connectors.
    pub funding_txid: Txid,

    /// Funding transaction output index.
    ///
    /// Used to cover the dust outputs in the transaction graph connectors.
    pub funding_vout: u32,

    /// Operator's X-only public key to construct a P2TR address to reimburse the
    /// operator for a valid withdraw fulfillment.
    // TODO: convert this a BOSD descriptor.
    pub operator_pk: XOnlyPublicKey,

    /// Winternitz One-Time Signature (WOTS) public keys shared in a deposit.
    pub wots_pks: WotsPublicKeys,

    /// Signature of the Operator's message using his P2P [`P2POperatorPubKey`].
    pub signature: Vec<u8>,

    /// The Operator's public key that the message came from.
    pub key: P2POperatorPubKey,
}
