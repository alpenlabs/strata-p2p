use async_trait::async_trait;
use bitcoin::{OutPoint, XOnlyPublicKey};
use libp2p_identity::PeerId;
use musig2::{PartialSignature, PubNonce};
use serde::{de::DeserializeOwned, Serialize};
use strata_p2p_types::{
    OperatorPubKey, Scope, SessionId, Wots160Key, Wots256Key, Wots32Key, WotsId,
};
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

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct AuthenticatedEntry<T> {
    pub entry: T,
    pub signature: Vec<u8>,
    pub key: OperatorPubKey,
}

pub type PartialSignaturesEntry = AuthenticatedEntry<Vec<PartialSignature>>;
pub type NoncesEntry = AuthenticatedEntry<Vec<PubNonce>>;
pub type GenesisInfoEntry = AuthenticatedEntry<(OutPoint, Vec<XOnlyPublicKey>)>;
pub type Wots32KeysEntry = AuthenticatedEntry<Vec<Wots32Key>>;
pub type Wots160KeysEntry = AuthenticatedEntry<Vec<Wots160Key>>;
pub type Wots256KeysEntry = AuthenticatedEntry<Vec<Wots256Key>>;

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

#[async_trait]
pub trait RepositoryExt<DepositSetupPayload>: Repository
where
    DepositSetupPayload: prost::Message + Default + Send + Sync + 'static,
{
    async fn get_partial_signatures(
        &self,
        operator_pk: &OperatorPubKey,
        session_id: SessionId,
    ) -> DBResult<Option<PartialSignaturesEntry>> {
        let key = format!("sigs-{operator_pk}_{session_id}");
        self.get(key).await
    }

    async fn set_partial_signatures_if_not_exists(
        &self,
        session_id: SessionId,
        entry: PartialSignaturesEntry,
    ) -> DBResult<bool> {
        let key = format!("sigs-{}_{session_id}", entry.key);
        self.set_if_not_exists(key, entry).await
    }

    /// Delete multiple entries of partial signatures from storage by pairs of
    /// operator pubkey and session ids.
    async fn delete_partial_signatures(
        &self,
        keys: &[(&OperatorPubKey, &SessionId)],
    ) -> DBResult<()> {
        let keys = keys
            .iter()
            .map(|(key, id)| format!("sigs-{key}_{id}"))
            .collect::<Vec<_>>();
        self.delete_raw(keys).await
    }

    async fn get_pub_nonces(
        &self,
        operator_pk: &OperatorPubKey,
        session_id: SessionId,
    ) -> DBResult<Option<NoncesEntry>> {
        let key = format!("nonces-{operator_pk}_{session_id}");
        self.get(key).await
    }

    async fn set_pub_nonces_if_not_exist(
        &self,
        session_id: SessionId,
        entry: NoncesEntry,
    ) -> DBResult<bool> {
        let key = format!("nonces-{}_{session_id}", entry.key);
        self.set_if_not_exists(key, entry).await
    }

    /// Delete multiple entries of pub nonces from storage by pairs of
    /// operator pubkey and session ids.
    async fn delete_pub_nonces(&self, keys: &[(&OperatorPubKey, &SessionId)]) -> DBResult<()> {
        let keys = keys
            .iter()
            .map(|(key, id)| format!("nonces-{key}_{id}"))
            .collect::<Vec<_>>();
        self.delete_raw(keys).await
    }

    async fn get_wots32_keys(
        &self,
        operator_pk: &OperatorPubKey,
        session_id: SessionId,
        wots_id: WotsId,
    ) -> DBResult<Option<Wots32KeysEntry>> {
        let key = format!("wots32-{operator_pk}_{session_id}_{wots_id}");
        self.get(key).await
    }

    async fn set_wots32_keys_if_not_exist(
        &self,
        session_id: SessionId,
        wots_id: WotsId,
        entry: Wots32KeysEntry,
    ) -> DBResult<bool> {
        let key = format!("wots32-{}_{session_id}_{wots_id}", entry.key);
        self.set_if_not_exists(key, entry).await
    }

    /// Delete multiple entries of wots 32 keys from storage by tuples of
    /// operator pubkey, session ids, and wots ids.
    async fn delete_wots32_keys(
        &self,
        keys: &[(&OperatorPubKey, &SessionId, &WotsId)],
    ) -> DBResult<()> {
        let keys = keys
            .iter()
            .map(|(key, id, wots_id)| format!("wots32-{key}_{id}_{wots_id}"))
            .collect::<Vec<_>>();
        self.delete_raw(keys).await
    }

    async fn get_wots160_keys(
        &self,
        operator_pk: &OperatorPubKey,
        session_id: SessionId,
        wots_id: WotsId,
    ) -> DBResult<Option<Wots160KeysEntry>> {
        let key = format!("wots160-{operator_pk}_{session_id}_{wots_id}");
        self.get(key).await
    }

    async fn set_wots160_keys_if_not_exist(
        &self,
        session_id: SessionId,
        wots_id: WotsId,
        entry: Wots160KeysEntry,
    ) -> DBResult<bool> {
        let key = format!("wots160-{}_{session_id}_{wots_id}", entry.key);
        self.set_if_not_exists(key, entry).await
    }

    /// Delete multiple entries of wots 160 keys from storage by tuples of
    /// operator pubkey, session ids, and wots ids.
    async fn delete_wots160_keys(
        &self,
        keys: &[(&OperatorPubKey, &SessionId, &WotsId)],
    ) -> DBResult<()> {
        let keys = keys
            .iter()
            .map(|(key, id, wots_id)| format!("wots160-{key}_{id}_{wots_id}"))
            .collect::<Vec<_>>();
        self.delete_raw(keys).await
    }

    async fn get_wots256_keys(
        &self,
        operator_pk: &OperatorPubKey,
        session_id: SessionId,
        wots_id: WotsId,
    ) -> DBResult<Option<Wots256KeysEntry>> {
        let key = format!("wots256-{operator_pk}_{session_id}_{wots_id}");
        self.get(key).await
    }

    async fn set_wots256_keys_if_not_exist(
        &self,
        session_id: SessionId,
        wots_id: WotsId,
        entry: Wots256KeysEntry,
    ) -> DBResult<bool> {
        let key = format!("wots256-{}_{session_id}_{wots_id}", entry.key);
        self.set_if_not_exists(key, entry).await
    }

    /// Delete multiple entries of wots 256 keys from storage by tuples of
    /// operator pubkey, session ids, and wots ids.
    async fn delete_wots256_keys(
        &self,
        keys: &[(&OperatorPubKey, &SessionId, &WotsId)],
    ) -> DBResult<()> {
        let keys = keys
            .iter()
            .map(|(key, id, wots_id)| format!("wots256-{key}_{id}_{wots_id}"))
            .collect::<Vec<_>>();
        self.delete_raw(keys).await
    }

    async fn get_deposit_setup(
        &self,
        operator_pk: &OperatorPubKey,
        scope: Scope,
    ) -> DBResult<Option<DepositSetupEntry<DepositSetupPayload>>> {
        let key = format!("setup-{operator_pk}_{scope}");
        self.get(key).await
    }

    async fn set_deposit_setup_if_not_exists(
        &self,
        scope: Scope,
        setup: DepositSetupEntry<DepositSetupPayload>,
    ) -> DBResult<bool> {
        let key = format!("setup-{}_{scope}", setup.key);
        self.set_if_not_exists(key, setup).await
    }

    /// Delete multiple entries of deposit setups from storage by pairs of
    /// operator pubkey and session ids.
    async fn delete_deposit_setups(&self, keys: &[(&OperatorPubKey, &Scope)]) -> DBResult<()> {
        let keys = keys
            .iter()
            .map(|(key, scope)| format!("setup-{key}_{scope}"))
            .collect::<Vec<_>>();
        self.delete_raw(keys).await
    }

    async fn get_genesis_info(
        &self,
        operator_pk: &OperatorPubKey,
    ) -> DBResult<Option<GenesisInfoEntry>> {
        let key = format!("genesis-{operator_pk}");
        self.get(key).await
    }

    async fn set_genesis_info_if_not_exists(&self, info: GenesisInfoEntry) -> DBResult<bool> {
        let key = format!("genesis-{}", info.key);
        self.set_if_not_exists(key, info).await
    }

    /* P2P stores mapping of Musig2 exchange signers (operators) to node peer
    id that publishes messages. These methods store and retrieve this
    mapping:  */

    /// Get peer id of node, that distributed message signed by operator
    /// pubkey.
    async fn get_peer_by_signer_pubkey(
        &self,
        operator_pk: &OperatorPubKey,
    ) -> DBResult<Option<PeerId>> {
        let key = format!("signers=>peerid-{}", operator_pk);
        self.get(key).await
    }

    /// Store peer id of node, that distributed message signed by this
    /// operator.
    async fn set_peer_for_signer_pubkey(
        &self,
        operator_pk: &OperatorPubKey,
        peer_id: PeerId,
    ) -> DBResult<()> {
        let key = format!("signers=>peerid-{}", operator_pk);
        self.set(key, peer_id).await
    }
}

impl<T, DSP> RepositoryExt<DSP> for T
where
    DSP: prost::Message + Default + Send + Sync + 'static,
    T: Repository,
{
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct DepositSetupEntry<DSP: prost::Message + Default> {
    #[serde(with = "prost_serde")]
    pub payload: DSP,
    pub signature: Vec<u8>,
    pub key: OperatorPubKey,
}
