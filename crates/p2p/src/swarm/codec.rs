//! This module implement protobuf serialization/deserizliation codec for request-response
//! behaviour.
//!
//! Copied from `rust-libp2p/protocols/request-response/src/json.rs` and
//! rewritten using [`prost`] crate.

use std::{io, marker::PhantomData};

use async_trait::async_trait;
use futures::prelude::*;
use libp2p::swarm::StreamProtocol;

/// Max request size in bytes.
// NOTE(Velnbur): commit f096394 in rust-libp2p repo made this one
// configurable recently, so we way want too.
const REQUEST_SIZE_MAXIMUM: u64 = 1024 * 1024;

/// Max response size in bytes.
const RESPONSE_SIZE_MAXIMUM: u64 = 10 * 1024 * 1024;

/// A [`Codec`] defines the request and response types
/// for a request-response [`Behaviour`](super::Behaviour) protocol or
/// protocol family and how they are encoded/decoded on an I/O stream.
pub struct Codec<Req, Resp> {
    /// Phatom data for the tuple request-response.
    phantom: PhantomData<(Req, Resp)>,
}

impl<Req, Resp> Default for Codec<Req, Resp> {
    fn default() -> Self {
        Codec {
            phantom: PhantomData,
        }
    }
}

impl<Req, Resp> Clone for Codec<Req, Resp> {
    fn clone(&self) -> Self {
        Self::default()
    }
}

#[async_trait]
impl<Req, Resp> libp2p::request_response::Codec for Codec<Req, Resp>
where
    Req: Send + prost::Message + Default,
    Resp: Send + prost::Message + Default,
{
    type Protocol = StreamProtocol;
    type Request = Req;
    type Response = Resp;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Req>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut vec = Vec::new();

        io.take(REQUEST_SIZE_MAXIMUM).read_to_end(&mut vec).await?;

        Ok(Req::decode(vec.as_slice())?)
    }

    async fn read_response<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Resp>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut vec = Vec::new();

        io.take(RESPONSE_SIZE_MAXIMUM).read_to_end(&mut vec).await?;

        Ok(Resp::decode(vec.as_slice())?)
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
        // TODO(Velnbur): reconsider his
        let mut buf = Vec::new();
        req.encode(&mut buf)?;

        io.write_all(&buf).await?;

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
        // TODO(Velnbur): reconsider his
        let mut buf = Vec::new();
        resp.encode(&mut buf)?;

        io.write_all(&buf).await?;

        Ok(())
    }
}
