//! Commands for P2P implementation from operator implemenation.

use bitcoin::{hashes::sha256, OutPoint, XOnlyPublicKey};
use musig2::{PartialSignature, PubNonce};
use prost::Message;

use crate::wire::p2p::v1::typed::{
    DepositNonces, DepositSetup, DepositSigs, GenesisInfo, GossipsubMsgDepositKind,
    GossipsubMsgKind,
};

/// Ask P2P implementation to distribute some data across network.
#[derive(Debug)]
#[allow(clippy::enum_variant_names)] /* remove this later, when other commands will be added */
pub(crate) enum Command<DepositSetupPayload> {
    SendGenesisInfo {
        pre_stake_outpoint: OutPoint,
        checkpoint_pubkeys: Vec<XOnlyPublicKey>,
    },
    SendDepositSetup {
        scope: sha256::Hash,
        payload: DepositSetupPayload,
    },
    SendDepositNonces {
        scope: sha256::Hash,
        pub_nonces: Vec<PubNonce>,
    },
    SendPartialSignatures {
        scope: sha256::Hash,
        partial_sigs: Vec<PartialSignature>,
    },
}

impl<DSP: Message> Command<DSP> {
    /// Returns `gossipsub_msg::Body` with concatenated data for further signing.
    pub(crate) fn into_gossipsub_msg(self) -> GossipsubMsgKind<DSP> {
        match self {
            Command::SendGenesisInfo {
                pre_stake_outpoint,
                checkpoint_pubkeys,
                ..
            } => GossipsubMsgKind::GenesisInfo(GenesisInfo {
                checkpoint_pubkeys,
                pre_stake_outpoint,
            }),
            Command::SendDepositSetup { scope, payload, .. } => GossipsubMsgKind::Deposit {
                scope,
                kind: GossipsubMsgDepositKind::Setup(DepositSetup { payload }),
            },
            Command::SendDepositNonces {
                scope, pub_nonces, ..
            } => GossipsubMsgKind::Deposit {
                scope,
                kind: GossipsubMsgDepositKind::Nonces(DepositNonces { nonces: pub_nonces }),
            },
            Command::SendPartialSignatures {
                scope,
                partial_sigs,
                ..
            } => GossipsubMsgKind::Deposit {
                scope,
                kind: GossipsubMsgDepositKind::Sigs(DepositSigs { partial_sigs }),
            },
        }
    }
}
