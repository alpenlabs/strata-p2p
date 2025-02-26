//! Types for the Strata P2P messaging protocol.
#![expect(incomplete_features)] // the generic_const_exprs feature is incomplete
#![feature(generic_const_exprs)] // but necessary for using const generic bounds in
#![feature(strict_overflow_ops)] // necessary for const wots computations

mod deposit_data;
mod operator;
mod scope;
mod session_id;
mod stake_chain_id;
mod stake_data;
mod wots;

pub use deposit_data::WotsPublicKeys;
pub use operator::OperatorPubKey;
pub use scope::Scope;
pub use session_id::SessionId;
pub use stake_chain_id::StakeChainId;
pub use stake_data::StakeData;
pub use wots::{Wots160PublicKey, Wots256PublicKey, WotsPublicKey, WOTS_SINGLE};
