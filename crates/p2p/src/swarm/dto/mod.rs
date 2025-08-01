//! Some structs that we use + serializing and deserializing.

pub(crate) mod signed_data;

#[cfg(feature = "kademlia")]
pub mod dht_record;

pub mod message;
