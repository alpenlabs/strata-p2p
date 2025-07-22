//! Entity to control P2P implementation, spawned in another async task,
//! and listen to its events and send commands through channels.

use std::{
    fmt::{self, Display},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{FutureExt, Stream};
use libp2p::{PeerId, identity::PublicKey};
use thiserror::Error;
use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        mpsc, oneshot,
    },
    time::timeout,
};
use tracing::warn;

#[cfg(feature = "request-response")]
use crate::events::ReqRespEvent;
use crate::{
    commands::{Command, QueryP2PStateCommand},
    events::GossipEvent,
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
}

#[cfg(feature = "request-response")]
impl ReqRespHandle {
    pub(crate) const fn new(events: mpsc::Receiver<ReqRespEvent>) -> Self {
        Self { events }
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
#[derive(Debug)]
pub struct GossipHandle {
    events: broadcast::Receiver<GossipEvent>,
}

impl GossipHandle {
    pub(crate) const fn new(events: broadcast::Receiver<GossipEvent>) -> Self {
        Self { events }
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
    commands: mpsc::Sender<Command>,
}

impl CommandHandle {
    pub(crate) const fn new(commands: mpsc::Sender<Command>) -> Self {
        Self { commands }
    }

    /// Sends command to P2P.
    pub async fn send_command(&self, command: impl Into<Command>) {
        let _ = self.commands.send(command.into()).await;
    }

    /// Checks if the P2P node is connected to the specified peer.
    /// Returns true if connected, false otherwise.
    pub async fn is_connected(&self, peer_id: PeerId) -> bool {
        let (sender, receiver) = oneshot::channel();

        // Send the command to check connection.
        let cmd = Command::QueryP2PState(QueryP2PStateCommand::IsConnected {
            transport_id: peer_id,
            response_sender: sender,
        });

        // Use a cloned sender to avoid borrow issues.
        let cmd_sender = self.commands.clone();

        // Send the command
        if cmd_sender.send(cmd).await.is_err() {
            // If the command channel is closed, assume not connected.
            return false;
        }

        // Wait for response with timeout.
        // TODO: make this configurable
        match timeout(Duration::from_secs(1), receiver).await {
            Ok(Ok(is_connected)) => is_connected,
            _ => false, // Timeout or channel closed
        }
    }

    /// Gets the list of all currently connected peers.
    pub async fn get_connected_peers(&self) -> Vec<PeerId> {
        let (sender, receiver) = oneshot::channel();

        // Send the command.
        let cmd = Command::QueryP2PState(QueryP2PStateCommand::GetConnectedPeers {
            response_sender: sender,
        });

        // Use a cloned sender to avoid borrow issues.
        let cmd_sender = self.commands.clone();

        // If sending fails, return empty list.
        if cmd_sender.send(cmd).await.is_err() {
            return Vec::new();
        }

        // Wait for response with timeout.
        // TODO: make this configurable
        match timeout(Duration::from_secs(1), receiver).await {
            Ok(Ok(peers)) => peers,
            _ => Vec::new(), // Timeout or channel closed
        }
    }

    /// Gets the app public key for a specific peer.
    /// Returns None if we are not connected to the peer or key exchange hasn't happened yet.
    pub async fn get_app_public_key(&self, peer_id: PeerId) -> Option<PublicKey> {
        let (sender, receiver) = oneshot::channel();

        // Send the command.
        let cmd = Command::QueryP2PState(QueryP2PStateCommand::GetAppPublicKey {
            transport_id: peer_id,
            response_sender: sender,
        });

        // Use a cloned sender to avoid borrow issues.
        let cmd_sender = self.commands.clone();

        // If sending fails, return None.
        if cmd_sender.send(cmd).await.is_err() {
            return None;
        }

        // Wait for response with timeout.
        // TODO: make this configurable
        match timeout(Duration::from_secs(1), receiver).await {
            Ok(Ok(app_key)) => app_key,
            _ => None, // Timeout or channel closed
        }
    }
}

impl Clone for GossipHandle {
    fn clone(&self) -> Self {
        Self::new(self.events.resubscribe())
    }
}

impl Clone for CommandHandle {
    fn clone(&self) -> Self {
        Self::new(self.commands.clone())
    }
}

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
    type Item = Option<ReqRespEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let poll = Box::pin(self.next_event()).poll_unpin(cx);
        match poll {
            Poll::Ready(Some(v)) => Poll::Ready(Some(Some(v))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
