pub(crate) mod common;

pub(crate) mod is_connected;
pub(crate) mod setup;

#[cfg(feature = "byos")]
pub(crate) mod setup_invalid_signature;

#[cfg(feature = "gossipsub")]
pub(crate) mod gossipsub;
#[cfg(feature = "gossipsub")]
pub(crate) mod new_op;
#[cfg(all(feature = "gossipsub", not(feature = "byos")))]
pub(crate) mod validator_integration;

#[cfg(feature = "quic")]
pub(crate) mod quic;

#[cfg(feature = "request-response")]
pub(crate) mod request_response;

#[cfg(feature = "kad")]
pub(crate) mod getclosestpeers;
#[cfg(feature = "kad")]
pub(crate) mod new_op_dht;
