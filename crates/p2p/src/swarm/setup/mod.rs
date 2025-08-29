//! This module implements the setup phase of P2P connections, handling exchange of application
//! public keys

pub mod behavior;
pub(crate) mod events;
pub(crate) mod flexbuffers_codec;
pub(crate) mod handler;
pub(crate) mod upgrade;
