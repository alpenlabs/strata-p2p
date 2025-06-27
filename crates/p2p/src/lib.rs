//! Strata P2P implementation.
#![expect(incomplete_features)] // the generic_const_exprs feature is incomplete
#![feature(generic_const_exprs)] // but necessary for using const generic bounds

pub mod commands;
pub mod events;
pub mod swarm;

#[cfg(test)]
mod tests;
