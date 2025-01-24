//! Commands for P2P implementation from operator implementation.

use bitcoin::{OutPoint, XOnlyPublicKey};
use libp2p::identity::secp256k1;
use musig2::{PartialSignature, PubNonce};
use prost::Message;
use strata_p2p_types::{OperatorPubKey, Scope, SessionId};
use strata_p2p_wire::p2p::v1::{
    DepositSetup, GenesisInfo, GetMessageRequest, GossipsubMsg, UnsignedGossipsubMsg,
};

/// Ask P2P implementation to distribute some data across network.
#[derive(Debug, Clone)]
pub enum Command<DepositSetupPayload> {
    /// Publish message through gossip sub network of peers.
    PublishMessage(PublishMessage<DepositSetupPayload>),

    /// Request some message directly from other operator by peer id.
    RequestMessage(GetMessageRequest),

    /// Clean session, scopes from internal DB.
    CleanStorage(CleanStorageCommand),
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
        scope: Scope,
        payload: DepositSetupPayload,
    },
    Musig2NoncesExchange {
        session_id: SessionId,
        pub_nonces: Vec<PubNonce>,
    },
    Musig2SignaturesExchange {
        session_id: SessionId,
        partial_sigs: Vec<PartialSignature>,
    },
}

impl<DSP: Message + Clone> From<PublishMessage<DSP>> for GossipsubMsg<DSP> {
    fn from(value: PublishMessage<DSP>) -> Self {
        GossipsubMsg {
            signature: value.signature,
            key: value.key,
            unsigned: value.msg.into(),
        }
    }
}

impl<DSP: Message + Default + Clone> UnsignedPublishMessage<DSP> {
    /// Sign `self` using supplied `keypair`. Returns a `Command`
    /// with resulting signature and public key from `keypair`.
    pub fn sign_secp256k1(&self, keypair: &secp256k1::Keypair) -> PublishMessage<DSP> {
        let kind: UnsignedGossipsubMsg<DSP> = self.clone().into();
        let msg = kind.content();
        let signature = keypair.secret().sign(&msg);

        PublishMessage {
            key: keypair.public().clone().into(),
            signature,
            msg: self.clone(),
        }
    }
}

impl<DSP: Message + Clone> From<UnsignedPublishMessage<DSP>> for UnsignedGossipsubMsg<DSP> {
    fn from(value: UnsignedPublishMessage<DSP>) -> Self {
        match value {
            UnsignedPublishMessage::GenesisInfo {
                pre_stake_outpoint,
                checkpoint_pubkeys,
            } => UnsignedGossipsubMsg::GenesisInfo(GenesisInfo {
                checkpoint_pubkeys,
                pre_stake_outpoint,
            }),
            UnsignedPublishMessage::DepositSetup { scope, payload } => {
                UnsignedGossipsubMsg::DepositSetup {
                    scope,
                    setup: DepositSetup { payload },
                }
            }
            UnsignedPublishMessage::Musig2NoncesExchange {
                session_id,
                pub_nonces,
            } => UnsignedGossipsubMsg::Musig2NoncesExchange {
                session_id,
                nonces: pub_nonces,
            },
            UnsignedPublishMessage::Musig2SignaturesExchange {
                session_id,
                partial_sigs,
            } => UnsignedGossipsubMsg::Musig2SignaturesExchange {
                session_id,
                signatures: partial_sigs,
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

/// Command P2P to clean entries from internal key-value storage by
/// session IDs, scopes and operator pubkeys.
#[derive(Debug, Clone)]
pub struct CleanStorageCommand {
    pub scopes: Vec<Scope>,
    pub session_ids: Vec<SessionId>,
    pub operators: Vec<OperatorPubKey>,
}

impl CleanStorageCommand {
    pub const fn new(
        scopes: Vec<Scope>,
        session_ids: Vec<SessionId>,
        operators: Vec<OperatorPubKey>,
    ) -> Self {
        Self {
            scopes,
            session_ids,
            operators,
        }
    }

    /// Clean entries only by scope and operators from storage.
    pub const fn with_scopes(scopes: Vec<Scope>, operators: Vec<OperatorPubKey>) -> Self {
        Self {
            scopes,
            session_ids: Vec::new(),
            operators,
        }
    }

    /// Clean entries only by session IDs and operators from storage.
    pub const fn with_session_ids(
        session_ids: Vec<SessionId>,
        operators: Vec<OperatorPubKey>,
    ) -> Self {
        Self {
            scopes: Vec::new(),
            session_ids,
            operators,
        }
    }
}

impl<DSP> From<CleanStorageCommand> for Command<DSP> {
    fn from(v: CleanStorageCommand) -> Self {
        Self::CleanStorage(v)
    }
}
