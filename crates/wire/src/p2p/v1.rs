pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/strata.bitvm2.p2p.v1.rs"));
}

pub(crate) mod typed;
pub use typed::*;
