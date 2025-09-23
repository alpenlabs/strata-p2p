//! Entity to control P2P implementation, spawned in another async task,
//! and listen to its events and send commands through channels.

use std::{
    fmt::{self, Display},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::Sink;
#[cfg(any(feature = "gossipsub", feature = "request-response"))]
use futures::{FutureExt, Stream};
#[cfg(not(feature = "byos"))]
use libp2p::PeerId;
#[cfg(feature = "byos")]
use libp2p::identity::PublicKey;
use thiserror::Error;
use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        mpsc,
        mpsc::error::SendError,
        oneshot,
    },
    time::timeout,
};
#[cfg(feature = "gossipsub")]
use tracing::warn;

#[cfg(feature = "request-response")]
use crate::commands::RequestResponseCommand;
#[cfg(feature = "request-response")]
use crate::events::ReqRespEvent;
#[cfg(feature = "gossipsub")]
use crate::{commands::GossipCommand, events::GossipEvent};
use crate::{
    commands::{Command, QueryP2PStateCommand},
    events::CommandEvent,
    swarm::default_handle_timeout,
};

/// The receiver lagged too far behind. Attempting to receive again will
/// return the oldest message still retained by the channel.
///
/// Includes the number of skipped messages.
#[derive(Debug, Clone, Error)]
pub struct ErrDroppedMsgs(u64);

impl Display for ErrDroppedMsgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GossipHandle dropped {} messages", self.0)
    }
}

/// Handle to receive a request-response event from P2P.
#[cfg(feature = "request-response")]
#[derive(Debug)]
pub struct ReqRespHandle {
    events: mpsc::Receiver<ReqRespEvent>,
    commands: mpsc::Sender<RequestResponseCommand>,
}

#[cfg(feature = "request-response")]
impl ReqRespHandle {
    pub(crate) const fn new(
        events: mpsc::Receiver<ReqRespEvent>,
        commands: mpsc::Sender<RequestResponseCommand>,
    ) -> Self {
        Self { events, commands }
    }

    /// Gets the next event from the P2P events channel.
    ///
    /// This handle can also be used as a [`Stream`] for convenient event processing.
    pub async fn next_event(&mut self) -> Option<ReqRespEvent> {
        self.events.recv().await
    }

    /// Checks if the event's channel is empty or not.
    pub fn events_is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Handle to receive a gossipsub event from P2P.
#[cfg(feature = "gossipsub")]
#[derive(Debug)]
pub struct GossipHandle {
    events: broadcast::Receiver<GossipEvent>,
    commands: mpsc::Sender<GossipCommand>,
}

#[cfg(feature = "gossipsub")]
impl GossipHandle {
    pub(crate) const fn new(
        events: broadcast::Receiver<GossipEvent>,
        commands: mpsc::Sender<GossipCommand>,
    ) -> Self {
        Self { events, commands }
    }

    /// Gets the next event from P2P from events channel.
    pub async fn next_event(&mut self) -> Result<GossipEvent, RecvError> {
        self.events.recv().await
    }

    /// Checks if the event's channel is empty or not.
    pub fn events_is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Handle to sends commands to P2P.
#[derive(Debug)]
pub struct CommandHandle {
    events: broadcast::Receiver<CommandEvent>,
    commands: mpsc::Sender<Command>,
}

impl CommandHandle {
    pub(crate) const fn new(
        events: broadcast::Receiver<CommandEvent>,
        commands: mpsc::Sender<Command>,
    ) -> Self {
        Self { commands, events }
    }

    /// Gets the next event from P2P from events channel.
    pub async fn next_event(&mut self) -> Result<CommandEvent, RecvError> {
        self.events.recv().await
    }

    /// Checks if the event's channel is empty or not.
    pub fn events_is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Sends command to P2P.
    pub async fn send_command(&self, command: impl Into<Command>) {
        let _ = self.commands.send(command.into()).await;
    }

    /// Get a new Command event receiver. Useful if it is necessary to get a receiver for
    /// `tokio_stream::BroadcastStream`.
    pub fn get_new_receiver(&self) -> broadcast::Receiver<CommandEvent> {
        self.events.resubscribe()
    }

    /// Checks if the P2P node is connected to the specified peer.
    /// If timeout is None, uses the default timeout of 1 second.
    /// Returns true if connected, false otherwise.
    #[cfg(feature = "byos")]
    pub async fn is_connected(
        &self,
        app_public_key: &PublicKey,
        timeout_duration: Option<Duration>,
    ) -> bool {
        let (sender, receiver) = oneshot::channel();

        let cmd = Command::QueryP2PState(QueryP2PStateCommand::IsConnected {
            app_public_key: app_public_key.clone(),
            response_sender: sender,
        });

        let duration = timeout_duration.unwrap_or(default_handle_timeout());
        let cmd_sender = self.commands.clone();

        if cmd_sender.send(cmd).await.is_err() {
            return false;
        }

        match timeout(duration, receiver).await {
            Ok(Ok(is_connected)) => is_connected,
            _ => false,
        }
    }

    /// Checks if the P2P node is connected to the specified peer by transport ID.
    /// If timeout is None, uses the default timeout of 1 second.
    /// Returns true if connected, false otherwise.
    #[cfg(not(feature = "byos"))]
    pub async fn is_connected(
        &self,
        transport_id: &PeerId,
        timeout_duration: Option<Duration>,
    ) -> bool {
        let (sender, receiver) = oneshot::channel();

        let cmd = Command::QueryP2PState(QueryP2PStateCommand::IsConnected {
            transport_id: *transport_id,
            response_sender: sender,
        });

        let duration = timeout_duration.unwrap_or(default_handle_timeout());
        let cmd_sender = self.commands.clone();

        if cmd_sender.send(cmd).await.is_err() {
            return false;
        }

        match timeout(duration, receiver).await {
            Ok(Ok(is_connected)) => is_connected,
            _ => false,
        }
    }

    /// Gets the list of all currently connected peers.
    ///
    /// If timeout is [`None`], uses the default timeout of 1 second.
    #[cfg(feature = "byos")]
    pub async fn get_connected_peers(&self, timeout_duration: Option<Duration>) -> Vec<PublicKey> {
        let (sender, receiver) = oneshot::channel();

        let cmd = Command::QueryP2PState(QueryP2PStateCommand::GetConnectedPeers {
            response_sender: sender,
        });

        let duration = timeout_duration.unwrap_or(default_handle_timeout());
        let cmd_sender = self.commands.clone();

        if cmd_sender.send(cmd).await.is_err() {
            return Vec::new();
        }

        match timeout(duration, receiver).await {
            Ok(Ok(peers)) => peers,
            _ => Vec::new(),
        }
    }

    /// Gets the list of all currently connected peers by transport ID.
    ///
    /// If timeout is [`None`], uses the default timeout of 1 second.
    #[cfg(not(feature = "byos"))]
    pub async fn get_connected_peers(&self, timeout_duration: Option<Duration>) -> Vec<PeerId> {
        let (sender, receiver) = oneshot::channel();

        let cmd = Command::QueryP2PState(QueryP2PStateCommand::GetConnectedPeers {
            response_sender: sender,
        });

        // TODO: make this configurable.
        let duration = timeout_duration.unwrap_or(default_handle_timeout());
        let cmd_sender = self.commands.clone();

        if cmd_sender.send(cmd).await.is_err() {
            return Vec::new();
        }

        match timeout(duration, receiver).await {
            Ok(Ok(peers)) => peers,
            _ => Vec::new(),
        }
    }
}

#[cfg(feature = "gossipsub")]
impl Clone for GossipHandle {
    fn clone(&self) -> Self {
        Self::new(self.events.resubscribe(), self.commands.clone())
    }
}

impl Clone for CommandHandle {
    fn clone(&self) -> Self {
        Self::new(self.events.resubscribe(), self.commands.clone())
    }
}

#[cfg(feature = "gossipsub")]
impl Stream for GossipHandle {
    type Item = Result<GossipEvent, ErrDroppedMsgs>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let poll = Box::pin(self.next_event()).poll_unpin(cx);
        match poll {
            Poll::Ready(Ok(v)) => Poll::Ready(Some(Ok(v))),
            Poll::Ready(Err(RecvError::Closed)) => Poll::Ready(None),
            Poll::Ready(Err(RecvError::Lagged(skipped))) => {
                warn!(%skipped, "Gossip Stream lost messages");
                Poll::Ready(Some(Err(ErrDroppedMsgs(skipped))))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(feature = "request-response")]
impl Stream for ReqRespHandle {
    type Item = ReqRespEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let poll = Box::pin(self.next_event()).poll_unpin(cx);
        match poll {
            Poll::Ready(Some(v)) => Poll::Ready(Some(v)),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(feature = "gossipsub")]
impl Sink<GossipCommand> for GossipHandle {
    type Error = SendError<GossipCommand>;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: GossipCommand) -> Result<(), Self::Error> {
        self.commands
            .try_send(item)
            .map_err(|e| SendError(e.into_inner()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(feature = "request-response")]
impl Sink<RequestResponseCommand> for ReqRespHandle {
    type Error = SendError<RequestResponseCommand>;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: RequestResponseCommand) -> Result<(), Self::Error> {
        self.commands
            .try_send(item)
            .map_err(|e| SendError(e.into_inner()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

impl Sink<Command> for CommandHandle {
    type Error = SendError<Command>;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Command) -> Result<(), Self::Error> {
        self.commands
            .try_send(item)
            .map_err(|e| SendError(e.into_inner()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}
