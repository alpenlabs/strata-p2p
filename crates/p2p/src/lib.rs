//! Strata P2P implementation.

#[cfg(all(feature = "kad", feature = "byos"))]
compile_error!("Enabling both \"kad\" any \"byos\" features is restricted.");

pub mod commands;
pub mod events;
pub mod swarm;

#[cfg(all(
    any(feature = "gossipsub", feature = "request-response"),
    not(feature = "byos")
))]
pub mod validator;

#[cfg(any(feature = "gossipsub", feature = "request-response", feature = "byos"))]
pub mod signer;

#[cfg(all(
    any(feature = "gossipsub", feature = "request-response"),
    not(feature = "byos")
))]
pub mod score_manager;

#[cfg(test)]
mod tests;
