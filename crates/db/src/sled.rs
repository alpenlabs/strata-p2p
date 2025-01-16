use std::sync::Arc;

use async_trait::async_trait;
use sled::Db;
use threadpool::ThreadPool;
use tokio::sync::oneshot;
use tracing::warn;

use super::{DBResult, Repository, RepositoryError};

pub struct AsyncDB {
    pool: ThreadPool,
    db: Arc<Db>,
}

impl AsyncDB {
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

        rx.await
            .map_err(|_| RepositoryError::KeyValueStorage {
                source: "Channel closed unexpectedly".into(),
            })?
            .map_err(|err| RepositoryError::KeyValueStorage { source: err.into() })
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

        rx.await
            .map_err(|_| RepositoryError::KeyValueStorage {
                source: "Channel closed unexpectedly".into(),
            })?
            .map_err(|err| RepositoryError::KeyValueStorage { source: err.into() })
    }
}

impl From<sled::Error> for RepositoryError {
    fn from(value: sled::Error) -> Self {
        RepositoryError::KeyValueStorage {
            source: value.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bitcoin::{
        OutPoint, XOnlyPublicKey,
        hashes::{Hash, sha256},
    };
    use musig2::{AggNonce, KeyAggContext, SecNonce, sign_partial};
    use rand::thread_rng;
    use secp256k1::{All, Keypair, Secp256k1};
    use strata_p2p_types::OperatorPubKey;

    use crate::{
        GenesisInfoEntry, NoncesEntry, PartialSignaturesEntry, RepositoryExt, sled::AsyncDB,
    };

    #[tokio::test]
    async fn test_repository() {
        let config = sled::Config::new().temporary(true);
        let db = config.open().expect("Failed to open sled database");
        let db = AsyncDB::new(Default::default(), Arc::new(db));

        async fn inner(db: &impl RepositoryExt<()>) {
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

            let operator_pk = OperatorPubKey(vec![0x8; 32]);
            let scope = sha256::Hash::all_zeros();

            let nonces_entry = NoncesEntry {
                entry: vec![pub_nonce.clone()],
                signature: vec![0x8; 32],
                key: operator_pk.clone(),
            };

            db.set_pub_nonces(scope, nonces_entry).await.unwrap();

            let agg_nonce = AggNonce::sum([pub_nonce.clone()]);
            let ctx = KeyAggContext::new([keypair.public_key()]).unwrap();

            let signature =
                sign_partial(&ctx, keypair.secret_key(), sec_nonce, &agg_nonce, message).unwrap();

            let sigs_entry = PartialSignaturesEntry {
                entry: vec![signature],
                signature: vec![],
                key: operator_pk.clone(),
            };

            db.set_partial_signatures(scope, sigs_entry)
                .await
                .expect("Failed to set signature");

            let retrieved_signature = db
                .get_partial_signatures(&operator_pk, scope)
                .await
                .unwrap()
                .expect("Failed to retrieve signature");

            assert_eq!(&retrieved_signature.entry, &[signature]);

            let outpoint = OutPoint::null();
            let checkpoint_pubkeys = vec![generate_random_xonly(&secp); 10000];
            let entry = GenesisInfoEntry {
                entry: (outpoint, checkpoint_pubkeys.clone()),
                signature: vec![],
                key: operator_pk.clone(),
            };

            db.set_genesis_info(entry).await.unwrap();

            let GenesisInfoEntry {
                entry: (got_op, got_keys),
                ..
            } = db.get_genesis_info(&operator_pk).await.unwrap().unwrap();
            assert_eq!(got_op, outpoint);
            assert_eq!(got_keys, checkpoint_pubkeys);

            let retrieved_pub_nonces = db
                .get_pub_nonces(&operator_pk, scope)
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
}
