//! Entity to control P2P implementation, spawned in another async task,
//! and listen to its events through channels.

use std::{
    fmt::Display,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures::{FutureExt, Stream};
use libp2p::{identity::secp256k1::Keypair, PeerId};
use thiserror::Error;
use tokio::{
    sync::{
        broadcast::{self, error::RecvError},
        mpsc, oneshot,
    },
    time::timeout,
};
use tracing::warn;

use crate::{
    commands::{Command, PublishMessage, QueryP2PStateCommand, UnsignedPublishMessage},
    events::Event,
};

#[derive(Debug, Clone, Error)]
pub struct ErrDroppedMsgs(u64);
impl Display for ErrDroppedMsgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "P2PHandle dropped {} messages", self.0)
    }
}

/// Handle to interact with P2P implementation spawned in another async
/// task. To create a new one, use [`super::P2P::new_handle`].
#[derive(Debug)]
pub struct P2PHandle {
    /// Event channel for the swarm.
    events: broadcast::Receiver<Event>,

    /// Command channel for the swarm.
    commands: mpsc::Sender<Command>,

    /// The Libp2p secp256k1 keypair used for signing messages.
    keypair: Keypair,
}

impl P2PHandle {
    /// Creates a new [`P2PHandle`].
    pub(crate) fn new(
        events: broadcast::Receiver<Event>,
        commands: mpsc::Sender<Command>,
        keypair: Keypair,
    ) -> Self {
        Self {
            events,
            commands,
            keypair,
        }
    }

    /// Sends command to P2P.
    pub async fn send_command(&self, command: impl Into<Command>) {
        let _ = self.commands.send(command.into()).await;
    }

    /// Gets the next event from P2P from events channel.
    pub async fn next_event(&mut self) -> Result<Event, RecvError> {
        self.events.recv().await
    }

    /// Checks if the event's channel is empty or not.
    pub fn events_is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Signs a message using the Libp2p secp256k1 keypair.
    pub fn sign_message(&self, msg: UnsignedPublishMessage) -> PublishMessage {
        msg.sign_secp256k1(&self.keypair)
    }

    /// Checks if the P2P node is connected to the specified peer.
    /// Returns true if connected, false otherwise.
    pub async fn is_connected(&self, peer_id: PeerId) -> bool {
        let (sender, receiver) = oneshot::channel();

        // Send the command to check connection
        let cmd = Command::QueryP2PState(QueryP2PStateCommand::IsConnected {
            peer_id,
            response_sender: sender,
        });

        // Use a cloned sender to avoid borrow issues
        let cmd_sender = self.commands.clone();

        // Send the command
        if cmd_sender.send(cmd).await.is_err() {
            // If the command channel is closed, assume not connected
            return false;
        }

        // Wait for response with timeout
        // TODO: make this configurable
        match timeout(Duration::from_secs(1), receiver).await {
            Ok(Ok(is_connected)) => is_connected,
            _ => false, // Timeout or channel closed
        }
    }

    /// Gets the list of all currently connected peers.
    pub async fn get_connected_peers(&self) -> Vec<PeerId> {
        let (sender, receiver) = oneshot::channel();

        // Send the command
        let cmd = Command::QueryP2PState(QueryP2PStateCommand::GetConnectedPeers {
            response_sender: sender,
        });

        // Use a cloned sender to avoid borrow issues
        let cmd_sender = self.commands.clone();

        // If sending fails, return empty list
        if cmd_sender.send(cmd).await.is_err() {
            return Vec::new();
        }

        // Wait for response with timeout
        // TODO: make this configurable
        match timeout(Duration::from_secs(1), receiver).await {
            Ok(Ok(peers)) => peers,
            _ => Vec::new(), // Timeout or channel closed
        }
    }
}

impl Clone for P2PHandle {
    fn clone(&self) -> Self {
        Self::new(
            self.events.resubscribe(),
            self.commands.clone(),
            self.keypair.clone(),
        )
    }
}

impl Stream for P2PHandle {
    type Item = Result<Event, ErrDroppedMsgs>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let poll = Box::pin(self.next_event()).poll_unpin(cx);
        match poll {
            Poll::Ready(Ok(v)) => Poll::Ready(Some(Ok(v))),
            Poll::Ready(Err(RecvError::Closed)) => Poll::Ready(None),
            Poll::Ready(Err(RecvError::Lagged(skipped))) => {
                warn!(%skipped, "P2P Stream lost messages");
                Poll::Ready(Some(Err(ErrDroppedMsgs(skipped))))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
