//! Entity to control P2P implementation, spawned in another async task,
//! and listen to its events through channels.

use futures::{FutureExt, Stream};
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

impl<DSP> Stream for P2PHandle<DSP>
where
    DSP: prost::Message + Clone,
{
    type Item = Event<DSP>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let poll = Box::pin(self.next_event()).poll_unpin(cx);
        match poll {
            std::task::Poll::Ready(Ok(v)) => std::task::Poll::Ready(Some(v)),
            std::task::Poll::Ready(Err(RecvError::Closed)) => std::task::Poll::Ready(None),
            // NOTE FOR REVIEW: should we be silently swallowing skipped messages here or should we
            // handle it differently?
            std::task::Poll::Ready(Err(RecvError::Lagged(_skipped))) => self.poll_next(cx),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}
