//! This module implement protobuf serialization/deserizliation codec for request-response
//! behaviour.
//!
//! Copied from `rust-libp2p/protocols/request-response/src/json.rs` and
//! rewritten so that it exposes raw bytes.

use std::{io, marker::PhantomData};

use async_trait::async_trait;
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

#[async_trait]
impl libp2p::request_response::Codec for Codec {
    type Protocol = StreamProtocol;
    type Request = Vec<u8>;
    type Response = Vec<u8>;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut vec = Vec::new();

        io.take(REQUEST_SIZE_MAXIMUM).read_to_end(&mut vec).await?;

        Ok(vec)
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut vec = Vec::new();

        io.take(RESPONSE_SIZE_MAXIMUM).read_to_end(&mut vec).await?;

        Ok(vec)
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        io.write_all(&req).await?;

        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        resp: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        io.write_all(&resp).await?;

        Ok(())
    }
}
