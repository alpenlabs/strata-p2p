//! Strata P2P implementation.

pub mod commands;
pub mod events;
pub mod swarm;

#[cfg(all(
    any(feature = "gossipsub", feature = "request-response"),
    not(feature = "byos")
))]
pub mod validator;

#[cfg(any(
    feature = "gossipsub",
    feature = "request-response",
    feature = "byos",
    feature = "kad"
))]
pub mod signer;

#[cfg(all(
    any(feature = "gossipsub", feature = "request-response"),
    not(feature = "byos")
))]
pub mod score_manager;

#[cfg(test)]
mod tests;
