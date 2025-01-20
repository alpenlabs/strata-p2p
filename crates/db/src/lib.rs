use async_trait::async_trait;
use bitcoin::{hashes::sha256, OutPoint, XOnlyPublicKey};
use musig2::{PartialSignature, PubNonce};
use serde::{de::DeserializeOwned, Serialize};
use strata_p2p_types::OperatorPubKey;
use thiserror::Error;

use crate::states::PeerDepositState;

pub mod sled;

pub mod states;

mod prost_serde;

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

    async fn set_partial_signatures(
        &self,
        scope: sha256::Hash,
        entry: PartialSignaturesEntry,
    ) -> DBResult<()> {
        let key = format!("sigs-{}_{scope}", entry.key);
        self.set(key, entry).await
    }

    async fn get_pub_nonces(
        &self,
        operator_pk: &OperatorPubKey,
        scope: sha256::Hash,
    ) -> DBResult<Option<NoncesEntry>> {
        let key = format!("nonces-{operator_pk}_{scope}");
        self.get(key).await
    }

    async fn set_pub_nonces(&self, scope: sha256::Hash, entry: NoncesEntry) -> DBResult<()> {
        let key = format!("nonces-{}_{scope}", entry.key);
        self.set(key, entry).await
    }

    async fn get_deposit_setup(
        &self,
        operator_pk: &OperatorPubKey,
        scope: sha256::Hash,
    ) -> DBResult<Option<DepositSetupEntry<DepositSetupPayload>>> {
        let key = format!("setup-{operator_pk}_{scope}");
        self.get(key).await
    }

    async fn set_deposit_setup(
        &self,
        scope: sha256::Hash,
        setup: DepositSetupEntry<DepositSetupPayload>,
    ) -> DBResult<()> {
        let key = format!("setup-{}_{scope}", setup.key);
        self.set(key, setup).await
    }

    async fn get_peer_deposit_status(
        &self,
        operator_pk: &OperatorPubKey,
        scope: sha256::Hash,
    ) -> DBResult<PeerDepositState> {
        if self
            .get_partial_signatures(operator_pk, scope)
            .await?
            .is_some()
        {
            return Ok(PeerDepositState::Sigs);
        }

        if self.get_pub_nonces(operator_pk, scope).await?.is_some() {
            return Ok(PeerDepositState::Nonces);
        }

        if self.get_deposit_setup(operator_pk, scope).await?.is_some() {
            return Ok(PeerDepositState::Setup);
        }

        Ok(PeerDepositState::PreSetup)
    }

    async fn get_genesis_info(
        &self,
        operator_pk: &OperatorPubKey,
    ) -> DBResult<Option<GenesisInfoEntry>> {
        let key = format!("genesis-{operator_pk}");
        self.get(key).await
    }

    async fn set_genesis_info(&self, info: GenesisInfoEntry) -> DBResult<()> {
        let key = format!("genesis-{}", info.key);
        self.set(key, info).await
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
