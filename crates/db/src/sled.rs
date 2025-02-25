//! [`Repository`] implementation using [`sled`] as a backend.

use std::sync::Arc;

use async_trait::async_trait;
use sled::Db;
use threadpool::ThreadPool;
use tokio::sync::{oneshot, oneshot::error::RecvError};
use tracing::warn;

use super::{DBResult, Repository, RepositoryError};

/// Thread-safe wrapper for [`Db`] with a [`ThreadPool`] for async operations.
#[derive(Clone)]
pub struct AsyncDB {
    /// Thread pool used for async operations.
    pool: ThreadPool,

    /// Thread-safe [`Db`] instance.
    db: Arc<Db>,
}

impl AsyncDB {
    /// Creates a new [`AsyncDB`] instance.
    pub fn new(pool: ThreadPool, db: Arc<Db>) -> Self {
        Self { pool, db }
    }
}

#[async_trait]
impl Repository for AsyncDB {
    async fn get_raw(&self, key: String) -> DBResult<Option<Vec<u8>>> {
        let (tx, rx) = oneshot::channel();

        let db = self.db.clone();
        self.pool.execute(move || {
            let value = sled::Tree::get(&db, key).map(|opt| opt.map(|v| v.to_vec()));

            if tx.send(value).is_err() {
                warn!("Receiver channel hanged up or dropped");
            }
        });

        rx.await?.map_err(Into::into)
    }

    async fn set_raw(&self, key: String, value: Vec<u8>) -> DBResult<()> {
        let (tx, rx) = oneshot::channel();

        let db = self.db.clone();
        self.pool.execute(move || {
            let value = db.insert(key, value).map(|_| ());

            if tx.send(value).is_err() {
                warn!("Receiver channel hanged up or dropped");
            }
        });

        rx.await?.map_err(Into::into)
    }

    async fn delete_raw(&self, keys: Vec<String>) -> DBResult<()> {
        let (tx, rx) = oneshot::channel();

        let db = self.db.clone();
        self.pool.execute(move || {
            let result = delete_all_by_key(db, keys);

            if tx.send(result).is_err() {
                warn!("Receiver channel hanged up or dropped");
            }
        });

        rx.await?.map_err(Into::into)
    }

    async fn set_raw_if_not_exists(&self, key: String, value: Vec<u8>) -> DBResult<bool> {
        let (tx, rx) = oneshot::channel();

        let db = self.db.clone();
        self.pool.execute(move || {
            let result = set_if_not_exist(db, key, value);

            if tx.send(result).is_err() {
                warn!("Receiver channel hanged up or dropped");
            }
        });

        rx.await?.map_err(Into::into)
    }
}

impl From<RecvError> for RepositoryError {
    fn from(value: RecvError) -> Self {
        RepositoryError::Storage(value.into())
    }
}

/// Helper function to set a value in a [`sled::Db`] if it doesn't exist.
fn set_if_not_exist(db: Arc<sled::Db>, key: String, value: Vec<u8>) -> Result<bool, sled::Error> {
    if db.get(key.clone())?.is_some() {
        return Ok(false);
    }

    db.insert(key, value)?;

    Ok(true)
}

/// Helper function to delete all values in a [`sled::Db`] with a given key.
fn delete_all_by_key(db: Arc<sled::Db>, keys: Vec<String>) -> Result<(), sled::Error> {
    for key in keys {
        db.remove(key)?;
    }

    Ok(())
}

impl From<sled::Error> for RepositoryError {
    fn from(value: sled::Error) -> Self {
        RepositoryError::Storage(value.into())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bitcoin::{
        hashes::{sha256, Hash},
        OutPoint, Txid, XOnlyPublicKey,
    };
    use musig2::{sign_partial, AggNonce, KeyAggContext, SecNonce};
    use rand::{thread_rng, Rng, RngCore};
    use secp256k1::{All, Keypair, Secp256k1};
    use strata_p2p_types::{OperatorPubKey, SessionId, StakeChainId, Wots256PublicKey};

    use crate::{
        sled::AsyncDB, NoncesEntry, PartialSignaturesEntry, RepositoryExt, StakeChainEntry,
        StakeData,
    };

    #[tokio::test]
    async fn test_repository() {
        let config = sled::Config::new().temporary(true);
        let db = config.open().expect("Failed to open sled database");
        let db = AsyncDB::new(Default::default(), Arc::new(db));

        async fn inner(db: &impl RepositoryExt) {
            let secp = Secp256k1::new();
            let keypair = Keypair::new(&secp, &mut rand::thread_rng());
            let message = b"message";

            let sec_nonce = SecNonce::generate(
                [0u8; 32],
                keypair.secret_key(),
                keypair.public_key(),
                message,
                [],
            );
            let pub_nonce = sec_nonce.public_nonce();

            let operator_pk = OperatorPubKey::from(vec![0x8; 32]);
            let session_id = SessionId::hash(b"session_id");

            let nonces_entry = NoncesEntry {
                entry: vec![pub_nonce.clone()],
                signature: vec![0x8; 32],
                key: operator_pk.clone(),
            };

            db.set_pub_nonces_if_not_exist(session_id, nonces_entry)
                .await
                .unwrap();

            let agg_nonce = AggNonce::sum([pub_nonce.clone()]);
            let ctx = KeyAggContext::new([keypair.public_key()]).unwrap();

            let signature =
                sign_partial(&ctx, keypair.secret_key(), sec_nonce, &agg_nonce, message).unwrap();

            let sigs_entry = PartialSignaturesEntry {
                entry: vec![signature],
                signature: vec![],
                key: operator_pk.clone(),
            };

            db.set_partial_signatures_if_not_exists(session_id, sigs_entry)
                .await
                .expect("Failed to set signature");

            let retrieved_signature = db
                .get_partial_signatures(&operator_pk, session_id)
                .await
                .unwrap()
                .expect("Failed to retrieve signature");

            assert_eq!(&retrieved_signature.entry, &[signature]);

            let stake_chain_id = StakeChainId::hash(b"stake_chain_id");
            let outpoint = OutPoint::null();
            let checkpoint_pubkeys = vec![generate_random_xonly(&secp); 10_000];
            let stake_data = vec![generate_random_stake_data(); 10_000];
            let entry = StakeChainEntry {
                entry: (outpoint, checkpoint_pubkeys.clone(), stake_data.clone()),
                signature: vec![],
                key: operator_pk.clone(),
            };

            db.set_stake_chain_info_if_not_exists(stake_chain_id, entry)
                .await
                .unwrap();

            let StakeChainEntry {
                entry: (got_op, got_keys, got_stake_data),
                ..
            } = db
                .get_stake_chain_info(&operator_pk, &stake_chain_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(got_op, outpoint);
            assert_eq!(got_keys, checkpoint_pubkeys);
            assert_eq!(got_stake_data, stake_data);

            let retrieved_pub_nonces = db
                .get_pub_nonces(&operator_pk, session_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&retrieved_pub_nonces.entry, &[pub_nonce]);
        }

        inner(&db).await
    }

    fn generate_random_xonly(ctx: &Secp256k1<All>) -> XOnlyPublicKey {
        let (_seckey, pubkey) = ctx.generate_keypair(&mut thread_rng());
        let (xonly, _parity) = pubkey.x_only_public_key();
        xonly
    }

    fn generate_random_wots() -> Wots256PublicKey {
        let mut rng = thread_rng();
        let mut wots = [[0; 20]; 256];
        for bytes in wots.iter_mut() {
            rng.fill_bytes(bytes);
        }
        Wots256PublicKey::new(wots)
    }

    fn generate_random_hash() -> sha256::Hash {
        let mut rng = thread_rng();
        let mut hash = [0; 32];
        rng.fill_bytes(&mut hash);
        sha256::Hash::from_byte_array(hash)
    }

    fn generate_random_outpoint() -> OutPoint {
        let mut rng = thread_rng();
        let txid = Txid::from_slice(&generate_random_hash().to_byte_array()).unwrap();
        let vout = rng.gen_range(0..u32::MAX);
        OutPoint::new(txid, vout)
    }

    fn generate_random_stake_data() -> StakeData {
        StakeData::new(
            generate_random_wots(),
            generate_random_hash(),
            generate_random_outpoint(),
        )
    }
}
