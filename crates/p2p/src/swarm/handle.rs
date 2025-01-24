//! Entity to control P2P implementation, spawned in another async task,
//! and listen to its events through channels.

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

    /// Send command to P2P.
    pub async fn send_command(&self, command: impl Into<Command<DSP>>) {
        let _ = self.commands.send(command.into()).await;
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
