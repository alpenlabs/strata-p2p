//! Types for the Strata P2P messaging protocol.
// #![expect(incomplete_features)] // the generic_const_exprs feature is incomplete
                                // #![feature(generic_const_exprs)] // but necessary for using const generic bounds in
                                // #![feature(strict_overflow_ops)] // necessary for const wots computations
                                // #![feature(const_strict_overflow_ops)] // necessary for const wots computations

mod operator;
mod scope;
mod session_id;

pub use operator::P2POperatorPubKey;
pub use scope::Scope;
pub use session_id::SessionId;
