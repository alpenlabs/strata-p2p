//! This module implement serialization/deserialization codec for [`libp2p::request_response`]
//! behaviour. Historically, this file was used to allow protobuf's encoding/decoding be done
//! inside [`libp2p`], and since we moved from any schemas, it just implements codec that gives raw
//! [`Vec<u8>`].
//!
//! Copied from `rust-libp2p/protocols/request-response/src/json.rs` and
//! rewritten so that it exposes raw bytes.

#![cfg(feature = "request-response")]

use std::{
    io,
    marker::PhantomData,
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
};

use futures::prelude::*;
use libp2p::{request_response, swarm::StreamProtocol};

/// Max request size in bytes (configurable via setter, defaults to 1 MiB).
static REQUEST_SIZE_MAXIMUM: AtomicU64 = AtomicU64::new(1_024 * 1_024);

/// Max response size in bytes (configurable via setter, defaults to 10 MiB).
static RESPONSE_SIZE_MAXIMUM: AtomicU64 = AtomicU64::new(10 * 1_024 * 1_024);

/// Update the maximum request size in bytes used by the request-response codec.
pub(crate) fn set_request_max_bytes(max: u64) {
    REQUEST_SIZE_MAXIMUM.store(max, Ordering::Relaxed);
}

/// Update the maximum response size in bytes used by the request-response codec.
#[cfg(feature = "request-response")]
pub(crate) fn set_response_max_bytes(max: u64) {
    RESPONSE_SIZE_MAXIMUM.store(max, Ordering::Relaxed);
}

/// A [`Codec`] defines the request and response types
/// for a request-response [`Behaviour`](super::Behaviour) protocol or
/// protocol family and how they are encoded/decoded on an I/O stream.
#[derive(Debug)]
#[cfg_attr(not(feature = "request-response"), allow(unreachable_pub))]
pub struct Codec {
    /// Phatom data for the tuple request-response.
    phantom: PhantomData<Vec<u8>>,
}

impl Default for Codec {
    fn default() -> Self {
        Codec {
            phantom: PhantomData,
        }
    }
}

impl Clone for Codec {
    fn clone(&self) -> Self {
        Self::default()
    }
}

// Note: the next code looks ugly because libp2p uses crate `async_trait` which is an old
// workaround of declaring `async fn` in a trait, and we moved away from using the crate
// since, technically speaking, from Rust 1.75.0 it's possible to write `async fn`. But
// because the old workaround and new way are not strictly compatible, we had to desugar it
// in a spectacular way because Rust has some problems with implicit lifetimes in
// such contexts.
#[cfg(feature = "request-response")]
impl request_response::Codec for Codec {
    type Protocol = StreamProtocol;
    type Request = Vec<u8>;
    type Response = Vec<u8>;

    fn read_request<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _: &'life1 Self::Protocol,
        io: &'life2 mut T,
    ) -> Pin<Box<dyn Future<Output = io::Result<Self::Request>> + Send + 'async_trait>>
    where
        T: AsyncRead + Unpin + Send,
        T: 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let mut vec = Vec::new();

            io.take(REQUEST_SIZE_MAXIMUM.load(Ordering::Relaxed))
                .read_to_end(&mut vec)
                .await?;

            Ok(vec)
        })
    }

    fn read_response<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _: &'life1 Self::Protocol,
        io: &'life2 mut T,
    ) -> Pin<Box<dyn Future<Output = io::Result<Self::Response>> + Send + 'async_trait>>
    where
        T: AsyncRead + Unpin + Send,
        T: 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let mut vec = Vec::new();

            io.take(RESPONSE_SIZE_MAXIMUM.load(Ordering::Relaxed))
                .read_to_end(&mut vec)
                .await?;

            Ok(vec)
        })
    }

    fn write_request<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _: &'life1 Self::Protocol,
        io: &'life2 mut T,
        req: Self::Request,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'async_trait>>
    where
        T: AsyncWrite + Unpin + Send,
        T: 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            io.write_all(&req).await?;

            Ok(())
        })
    }

    fn write_response<'life0, 'life1, 'life2, 'async_trait, T>(
        &'life0 mut self,
        _: &'life1 Self::Protocol,
        io: &'life2 mut T,
        resp: Self::Response,
    ) -> Pin<Box<dyn Future<Output = io::Result<()>> + Send + 'async_trait>>
    where
        T: AsyncWrite + Unpin + Send,
        T: 'async_trait,
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            io.write_all(&resp).await?;

            Ok(())
        })
    }
}
