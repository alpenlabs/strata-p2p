use bitcoin::{hashes::sha256, OutPoint, XOnlyPublicKey};
use musig2::{PartialSignature, PubNonce};
use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc,
};

use crate::{commands::Command, events::Event};

#[derive(Debug)]
pub struct P2PHandle<DSP>
where
    DSP: prost::Message + Clone,
{
    events: broadcast::Receiver<Event<DSP>>,
    commands: mpsc::Sender<Command<DSP>>,
}

impl<DSP> P2PHandle<DSP>
where
    DSP: prost::Message + Clone,
{
    pub(crate) fn new(
        events: broadcast::Receiver<Event<DSP>>,
        commands: mpsc::Sender<Command<DSP>>,
    ) -> Self {
        Self { events, commands }
    }

    pub async fn send_genesis_info(
        &self,
        pre_stake_outpoint: OutPoint,
        checkpoint_pubkeys: Vec<XOnlyPublicKey>,
    ) {
        let _ = self
            .commands
            .send(Command::SendGenesisInfo {
                pre_stake_outpoint,
                checkpoint_pubkeys,
            })
            .await;
    }

    pub async fn send_deposit_setup(&self, scope: sha256::Hash, payload: DSP) {
        let _ = self
            .commands
            .send(Command::SendDepositSetup { scope, payload })
            .await;
    }

    pub async fn send_deposit_nonces(&self, scope: sha256::Hash, pub_nonces: Vec<PubNonce>) {
        let _ = self
            .commands
            .send(Command::SendDepositNonces { scope, pub_nonces })
            .await;
    }

    pub async fn send_deposit_sigs(
        &self,
        scope: sha256::Hash,
        partial_sigs: Vec<PartialSignature>,
    ) {
        let _ = self
            .commands
            .send(Command::SendPartialSignatures {
                scope,
                partial_sigs,
            })
            .await;
    }

    pub async fn next_event(&mut self) -> Result<Event<DSP>, RecvError> {
        self.events.recv().await
    }

    pub fn events_is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
