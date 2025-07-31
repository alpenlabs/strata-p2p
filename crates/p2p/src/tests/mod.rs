pub(crate) mod common;

pub(crate) mod is_connected;
pub(crate) mod setup;
pub(crate) mod setup_invalid_signature;

#[cfg(feature = "gossipsub")]
pub(crate) mod gossipsub;
#[cfg(feature = "gossipsub")]
pub(crate) mod new_op;

#[cfg(feature = "quic")]
pub(crate) mod quic;

#[cfg(feature = "request-response")]
pub(crate) mod request_response;

#[cfg(all(feature = "request-response", feature = "kademlia"))]
pub(crate) mod dht_request_response;

#[cfg(feature = "kademlia")]
pub(crate) mod dht_record;
