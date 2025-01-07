use bitcoin::hashes::{sha256, Hash};
use libp2p::gossipsub::{Hasher, TopicHash};

pub struct Sha256Hasher;

impl Hasher for Sha256Hasher {
    fn hash(topic_string: String) -> libp2p::gossipsub::TopicHash {
        let hash = sha256::Hash::hash(topic_string.as_bytes()).to_string();

        TopicHash::from_raw(hash)
    }
}
