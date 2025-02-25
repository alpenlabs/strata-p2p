//! Entity to control P2P implementation, spawned in another async task,
//! and listen to its events through channels.

use std::fmt::Display;

use futures::{FutureExt, Stream};
use thiserror::Error;
use tokio::sync::{
    broadcast::{self, error::RecvError},
    mpsc,
};

use crate::{commands::Command, events::Event};

#[derive(Debug, Clone, Error)]
pub struct ErrDroppedMsgs(u64);
impl Display for ErrDroppedMsgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "P2PHandle dropped {} messages", self.0)
    }
}

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
    type Item = Result<Event<DSP>, ErrDroppedMsgs>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        let poll = Box::pin(self.next_event()).poll_unpin(cx);
        match poll {
            std::task::Poll::Ready(Ok(v)) => std::task::Poll::Ready(Some(Ok(v))),
            std::task::Poll::Ready(Err(RecvError::Closed)) => std::task::Poll::Ready(None),
            std::task::Poll::Ready(Err(RecvError::Lagged(skipped))) => {
                tracing::warn!("P2P Stream lost {} messages", skipped);
                std::task::Poll::Ready(Some(Err(ErrDroppedMsgs(skipped))))
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}
