pub(crate) mod common;

pub(crate) mod is_connected;

#[cfg(feature = "byos")]
pub(crate) mod setup;

#[cfg(feature = "byos")]
pub(crate) mod setup_invalid_signature;

#[cfg(feature = "gossipsub")]
pub(crate) mod gossipsub;

#[cfg(feature = "gossipsub")]
pub(crate) mod new_op;

#[cfg(all(
    any(feature = "gossipsub", feature = "request-response"),
    not(feature = "byos")
))]
pub(crate) mod validator_integration;

#[cfg(feature = "quic")]
pub(crate) mod quic;

#[cfg(feature = "request-response")]
pub(crate) mod request_response;

#[cfg(any(feature = "kad", feature = "gossipsub", feature = "request-response"))]
pub(crate) mod flexbuffers;

#[cfg(feature = "kad")]
pub(crate) mod new_op_dht;

pub(crate) mod conn_limits;

#[cfg(feature = "mem-conn-limits-abs")]
pub(crate) mod mem_abs_conn_limits;

#[cfg(feature = "mem-conn-limits-rel")]
pub(crate) mod mem_rel_conn_limits;

#[cfg(feature = "kad")]
pub(crate) mod find_multiaddr;

#[cfg(all(
    feature = "kad",
    not(any(feature = "byos", feature = "gossipsub", feature = "request-response"))
))]
pub(crate) mod kad_invalid_record;

#[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
pub(crate) mod protocol_support_minimal;
