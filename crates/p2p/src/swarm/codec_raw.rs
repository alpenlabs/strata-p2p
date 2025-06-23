//! This module implement protobuf serialization/deserizliation codec for request-response
//! behaviour.
//!
//! Copied from `rust-libp2p/protocols/request-response/src/json.rs` and
//! rewritten so that it exposes raw bytes.

use std::{io, marker::PhantomData, pin::Pin};

use futures::prelude::*;
use libp2p::swarm::StreamProtocol;

/// Max request size in bytes.
// NOTE(Velnbur): commit f096394 in rust-libp2p repo made this one
// configurable recently, so we may want to configure it too.
const REQUEST_SIZE_MAXIMUM: u64 = 1024 * 1024;

/// Max response size in bytes.
const RESPONSE_SIZE_MAXIMUM: u64 = 10 * 1024 * 1024;

/// A [`Codec`] defines the request and response types
/// for a request-response [`Behaviour`](super::Behaviour) protocol or
/// protocol family and how they are encoded/decoded on an I/O stream.
#[derive(Debug)]
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

impl libp2p::request_response::Codec for Codec {
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

            io.take(REQUEST_SIZE_MAXIMUM).read_to_end(&mut vec).await?;

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

            io.take(RESPONSE_SIZE_MAXIMUM).read_to_end(&mut vec).await?;

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
