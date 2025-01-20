use async_trait::async_trait;
use bitcoin::{hashes::sha256, OutPoint, XOnlyPublicKey};
use libp2p_identity::PeerId;
use musig2::{PartialSignature, PubNonce};
use serde::{de::DeserializeOwned, Serialize};
use strata_p2p_types::OperatorPubKey;
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

#[async_trait]
pub trait Repository: Send + Sync + 'static {
    async fn get_raw(&self, key: String) -> DBResult<Option<Vec<u8>>>;
    async fn set_raw(&self, key: String, value: Vec<u8>) -> DBResult<()>;

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
        scope: sha256::Hash,
    ) -> DBResult<Option<PartialSignaturesEntry>> {
        let key = format!("sigs-{operator_pk}_{scope}");
        self.get(key).await
    }

    async fn set_partial_signatures_if_not_exists(
        &self,
        scope: sha256::Hash,
        entry: PartialSignaturesEntry,
    ) -> DBResult<bool> {
        let key = format!("sigs-{}_{scope}", entry.key);
        self.set_if_not_exists(key, entry).await
    }

    async fn get_pub_nonces(
        &self,
        operator_pk: &OperatorPubKey,
        scope: sha256::Hash,
    ) -> DBResult<Option<NoncesEntry>> {
        let key = format!("nonces-{operator_pk}_{scope}");
        self.get(key).await
    }

    async fn set_pub_nonces_if_not_exist(
        &self,
        scope: sha256::Hash,
        entry: NoncesEntry,
    ) -> DBResult<bool> {
        let key = format!("nonces-{}_{scope}", entry.key);
        self.set_if_not_exists(key, entry).await
    }

    async fn get_deposit_setup(
        &self,
        operator_pk: &OperatorPubKey,
        scope: sha256::Hash,
    ) -> DBResult<Option<DepositSetupEntry<DepositSetupPayload>>> {
        let key = format!("setup-{operator_pk}_{scope}");
        self.get(key).await
    }

    async fn set_deposit_setup_if_not_exists(
        &self,
        scope: sha256::Hash,
        setup: DepositSetupEntry<DepositSetupPayload>,
    ) -> DBResult<bool> {
        let key = format!("setup-{}_{scope}", setup.key);
        self.set_if_not_exists(key, setup).await
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
