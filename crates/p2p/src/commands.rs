//! Commands for P2P implementation from operator implementation.

use bitcoin::{hashes::sha256, OutPoint, XOnlyPublicKey};
use libp2p::identity::secp256k1;
use musig2::{PartialSignature, PubNonce};
use prost::Message;
use strata_p2p_types::OperatorPubKey;
use strata_p2p_wire::p2p::v1::{
    DepositNonces, DepositSetup, DepositSigs, GenesisInfo, GetMessageRequest, GossipsubMsg,
    GossipsubMsgDepositKind, GossipsubMsgKind,
};

/// Ask P2P implementation to distribute some data across network.
#[derive(Debug, Clone)]
pub enum Command<DepositSetupPayload> {
    /// Publish message through gossip sub network of peers.
    PublishMessage(PublishMessage<DepositSetupPayload>),

    /// Request some message directly from other operator by peer id.
    RequestMessage(GetMessageRequest),
}

#[derive(Debug, Clone)]
pub struct PublishMessage<DepositSetupPayload> {
    pub key: OperatorPubKey,
    pub signature: Vec<u8>,
    pub msg: UnsignedPublishMessage<DepositSetupPayload>,
}

#[derive(Debug, Clone)]
pub enum UnsignedPublishMessage<DepositSetupPayload> {
    GenesisInfo {
        pre_stake_outpoint: OutPoint,
        checkpoint_pubkeys: Vec<XOnlyPublicKey>,
    },
    DepositSetup {
        scope: sha256::Hash,
        payload: DepositSetupPayload,
    },
    DepositNonces {
        scope: sha256::Hash,
        pub_nonces: Vec<PubNonce>,
    },
    PartialSignatures {
        scope: sha256::Hash,
        partial_sigs: Vec<PartialSignature>,
    },
}

impl<DSP: Message + Clone> From<PublishMessage<DSP>> for GossipsubMsg<DSP> {
    fn from(value: PublishMessage<DSP>) -> Self {
        GossipsubMsg {
            signature: value.signature,
            key: value.key,
            kind: value.msg.into(),
        }
    }
}

impl<DSP: Message + Default + Clone> UnsignedPublishMessage<DSP> {
    /// Sign `self` using supplied `keypair`. Returns a `Command`
    /// with resulting signature and public key from `keypair`.
    pub fn sign_secp256k1(&self, keypair: &secp256k1::Keypair) -> PublishMessage<DSP> {
        let kind: GossipsubMsgKind<DSP> = self.clone().into();
        let msg = kind.content();
        let signature = keypair.secret().sign(&msg);

        PublishMessage {
            key: keypair.public().clone().into(),
            signature,
            msg: self.clone(),
        }
    }
}

impl<DSP: Message + Clone> From<UnsignedPublishMessage<DSP>> for GossipsubMsgKind<DSP> {
    fn from(value: UnsignedPublishMessage<DSP>) -> Self {
        match value {
            UnsignedPublishMessage::GenesisInfo {
                pre_stake_outpoint,
                checkpoint_pubkeys,
            } => GossipsubMsgKind::GenesisInfo(GenesisInfo {
                checkpoint_pubkeys,
                pre_stake_outpoint,
            }),
            UnsignedPublishMessage::DepositSetup { scope, payload } => GossipsubMsgKind::Deposit {
                scope,
                kind: GossipsubMsgDepositKind::Setup(DepositSetup { payload }),
            },
            UnsignedPublishMessage::DepositNonces { scope, pub_nonces } => {
                GossipsubMsgKind::Deposit {
                    scope,
                    kind: GossipsubMsgDepositKind::Nonces(DepositNonces { nonces: pub_nonces }),
                }
            }
            UnsignedPublishMessage::PartialSignatures {
                scope,
                partial_sigs,
            } => GossipsubMsgKind::Deposit {
                scope,
                kind: GossipsubMsgDepositKind::Sigs(DepositSigs { partial_sigs }),
            },
        }
    }
}

impl<DepositSetupPayload> From<PublishMessage<DepositSetupPayload>>
    for Command<DepositSetupPayload>
{
    fn from(v: PublishMessage<DepositSetupPayload>) -> Self {
        Self::PublishMessage(v)
    }
}
