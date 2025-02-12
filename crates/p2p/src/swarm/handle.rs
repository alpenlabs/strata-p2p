//! Entity to control P2P implementation, spawned in another async task,
//! and listen to its events through channels.

use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc,
};

use crate::{commands::Command, events::Event};

/// Handle to interact with P2P implementation spawned in another async
/// task. To create a new one, use [`super::P2P::new_handle`].
#[derive(Debug)]
pub struct P2PHandle<DSP>
where
    DSP: prost::Message + Clone,
{
    /// Event channel for the swarm.
    events: broadcast::Receiver<Event<DSP>>,

    /// Command channel for the swarm.
    commands: mpsc::Sender<Command<DSP>>,
}

impl<DSP> P2PHandle<DSP>
where
    DSP: prost::Message + Clone,
{
    /// Creates a new [`P2PHandle`].
    pub(crate) fn new(
        events: broadcast::Receiver<Event<DSP>>,
        commands: mpsc::Sender<Command<DSP>>,
    ) -> Self {
        Self { events, commands }
    }

    /// Sends command to P2P.
    pub async fn send_command(&self, command: impl Into<Command<DSP>>) {
        let _ = self.commands.send(command.into()).await;
    }

    /// Gets the next event from P2P from events channel.
    pub async fn next_event(&mut self) -> Result<Event<DSP>, RecvError> {
        self.events.recv().await
    }

    /// Checks if the event's channel is empty or not.
    pub fn events_is_empty(&self) -> bool {
        self.events.is_empty()
    }
}
