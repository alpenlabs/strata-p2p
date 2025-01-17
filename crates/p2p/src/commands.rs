//! Commands for P2P implementation from operator implementation.

use bitcoin::{hashes::sha256, OutPoint, XOnlyPublicKey};
use libp2p::identity::secp256k1;
use musig2::{PartialSignature, PubNonce};
use prost::Message;
use strata_p2p_types::OperatorPubKey;
use strata_p2p_wire::p2p::v1::{
    DepositNonces, DepositSetup, DepositSigs, GenesisInfo, GossipsubMsg, GossipsubMsgDepositKind,
    GossipsubMsgKind,
};

/// Ask P2P implementation to distribute some data across network.
#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)] /* remove this later, when other commands will be added */
pub struct Command<DepositSetupPayload> {
    pub key: OperatorPubKey,
    pub signature: Vec<u8>,
    pub kind: CommandKind<DepositSetupPayload>,
}

#[derive(Debug, Clone)]
pub enum CommandKind<DepositSetupPayload> {
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

impl<DSP: Message + Clone> From<Command<DSP>> for GossipsubMsg<DSP> {
    fn from(value: Command<DSP>) -> Self {
        GossipsubMsg {
            signature: value.signature,
            key: value.key,
            kind: value.kind.into(),
        }
    }
}

impl<DSP: Message + Default + Clone> CommandKind<DSP> {
    /// Sign `self` using supplied `keypair`. Returns a `Command`
    /// with resulting signature and public key from `keypair`.
    pub fn sign_secp256k1(&self, keypair: &secp256k1::Keypair) -> Command<DSP> {
        let kind: GossipsubMsgKind<DSP> = self.clone().into();
        let msg = kind.content();
        let signature = keypair.secret().sign(&msg);
        Command {
            key: keypair.public().clone().into(),
            signature,
            kind: self.clone(),
        }
    }
}

impl<DSP: Message + Clone> From<CommandKind<DSP>> for GossipsubMsgKind<DSP> {
    fn from(value: CommandKind<DSP>) -> Self {
        match value {
            CommandKind::SendGenesisInfo {
                pre_stake_outpoint,
                checkpoint_pubkeys,
            } => GossipsubMsgKind::GenesisInfo(GenesisInfo {
                checkpoint_pubkeys,
                pre_stake_outpoint,
            }),
            CommandKind::SendDepositSetup { scope, payload } => GossipsubMsgKind::Deposit {
                scope,
                kind: GossipsubMsgDepositKind::Setup(DepositSetup { payload }),
            },
            CommandKind::SendDepositNonces { scope, pub_nonces } => GossipsubMsgKind::Deposit {
                scope,
                kind: GossipsubMsgDepositKind::Nonces(DepositNonces { nonces: pub_nonces }),
            },
            CommandKind::SendPartialSignatures {
                scope,
                partial_sigs,
            } => GossipsubMsgKind::Deposit {
                scope,
                kind: GossipsubMsgDepositKind::Sigs(DepositSigs { partial_sigs }),
            },
        }
    }
}
