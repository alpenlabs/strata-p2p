use async_trait::async_trait;

use super::{DBResult, Repository, RepositoryError};

#[async_trait]
impl Repository for sled::Db {
    async fn get_raw(&self, key: String) -> DBResult<Option<Vec<u8>>> {
        let value = sled::Tree::get(self, key)?;

        Ok(value.map(|v| v.to_vec()))
    }
    async fn set_raw(&self, key: String, value: Vec<u8>) -> DBResult<()> {
        self.insert(key, value)?;

        Ok(())
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
    use bitcoin::{
        OutPoint, XOnlyPublicKey,
        hashes::{Hash, sha256},
    };
    use libp2p_identity::PeerId;
    use musig2::{AggNonce, KeyAggContext, SecNonce, sign_partial};
    use rand::thread_rng;
    use secp256k1::{All, Keypair, Secp256k1};

    use crate::{
        GenesisInfoEntry, NoncesEntry, PartialSignaturesEntry, RepositoryExt, SerializablePublicKey,
    };

    #[tokio::test]
    async fn test_repository() {
        let config = sled::Config::new().temporary(true);
        let db = config.open().expect("Failed to open sled database");

        async fn inner(db: &impl RepositoryExt<()>) {
            let secp = Secp256k1::new();
            let keypair = Keypair::new(&secp, &mut rand::thread_rng());
            let message = b"message";
            let libp2p_pkey = generate_random_pubkey();

            let sec_nonce = SecNonce::generate(
                [0u8; 32],
                keypair.secret_key(),
                keypair.public_key(),
                message,
                [],
            );
            let pub_nonce = sec_nonce.public_nonce();

            let operator_id = PeerId::random();
            let tx_id = sha256::Hash::all_zeros();

            let nonces_entry = NoncesEntry {
                entry: vec![pub_nonce.clone()],
                signature: vec![0x8; 32],
                key: libp2p_pkey.clone(),
            };

            db.set_pub_nonces(operator_id, tx_id, nonces_entry)
                .await
                .unwrap();

            let agg_nonce = AggNonce::sum([pub_nonce.clone()]);
            let ctx = KeyAggContext::new([keypair.public_key()]).unwrap();

            let signature =
                sign_partial(&ctx, keypair.secret_key(), sec_nonce, &agg_nonce, message).unwrap();

            let sigs_entry = PartialSignaturesEntry {
                entry: vec![signature],
                signature: vec![],
                key: libp2p_pkey.clone(),
            };

            db.set_partial_signatures(operator_id, tx_id, sigs_entry)
                .await
                .expect("Failed to set signature");

            let retrieved_signature = db
                .get_partial_signatures(operator_id, tx_id)
                .await
                .unwrap()
                .expect("Failed to retrieve signature");

            assert_eq!(&retrieved_signature.entry, &[signature]);

            let outpoint = OutPoint::null();
            let checkpoint_pubkeys = vec![generate_random_xonly(&secp); 10000];
            let entry = GenesisInfoEntry {
                entry: (outpoint, checkpoint_pubkeys.clone()),
                signature: vec![],
                key: libp2p_pkey.clone(),
            };

            db.set_genesis_info(operator_id, entry).await.unwrap();

            let GenesisInfoEntry {
                entry: (got_op, got_keys),
                ..
            } = db.get_genesis_info(operator_id).await.unwrap().unwrap();
            assert_eq!(got_op, outpoint);
            assert_eq!(got_keys, checkpoint_pubkeys);

            let retrieved_pub_nonces = db
                .get_pub_nonces(operator_id, tx_id)
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

    fn generate_random_pubkey() -> SerializablePublicKey {
        let keypair = libp2p_identity::secp256k1::Keypair::generate();
        let pk = keypair.public().clone();

        SerializablePublicKey::from(pk)
    }
}
