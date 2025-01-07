//! This module implement protobuf serialization/deserizliation codec for request-response
//! behaviour.

use std::{io, marker::PhantomData};

use async_trait::async_trait;
use futures::prelude::*;
use libp2p::swarm::StreamProtocol;

/// Max request size in bytes
const REQUEST_SIZE_MAXIMUM: u64 = 1024 * 1024;
/// Max response size in bytes
const RESPONSE_SIZE_MAXIMUM: u64 = 10 * 1024 * 1024;

pub struct Codec<Req, Resp> {
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
