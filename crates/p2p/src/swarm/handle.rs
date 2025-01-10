//! Entity to control P2P implementation, spawned in another async task,
//! and listem to its events through channels.

use bitcoin::{hashes::sha256, OutPoint, XOnlyPublicKey};
use musig2::{PartialSignature, PubNonce};
use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc,
};

use crate::{commands::Command, events::Event};

/// Handle to interact with P2P implementation spawned in another async
/// task. To create new one, use [`super::P2P::new_handle`].
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

    /// Send command for P2P implementation to distribute genesis info across
    /// network.
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

    /// Send command for P2P implementation to distribute deposit setup
    /// across network.
    pub async fn send_deposit_setup(&self, scope: sha256::Hash, payload: DSP) {
        let _ = self
            .commands
            .send(Command::SendDepositSetup { scope, payload })
            .await;
    }

    /// Send command for P2P implementation to distribute deposit nonces
    /// across network.
    pub async fn send_deposit_nonces(&self, scope: sha256::Hash, pub_nonces: Vec<PubNonce>) {
        let _ = self
            .commands
            .send(Command::SendDepositNonces { scope, pub_nonces })
            .await;
    }

    /// Send command for P2P implementation to distribute sigs nonces across
    /// network.
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

    /// Get next event from P2P from events channel.
    pub async fn next_event(&mut self) -> Result<Event<DSP>, RecvError> {
        self.events.recv().await
    }

    /// Check event's channel is empty or not.
    pub fn events_is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
