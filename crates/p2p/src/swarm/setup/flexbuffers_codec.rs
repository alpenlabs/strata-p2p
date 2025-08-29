use std::io;

use asynchronous_codec::{Decoder, Encoder};
use bytes::{BufMut, BytesMut};
use serde::{Deserialize, Serialize};

/// A codec for flexbuffers-based encoding/decoding of messages.
#[derive(Clone, Debug, Default)]
pub(crate) struct FlexbuffersCodec<T> {
    _marker: std::marker::PhantomData<T>,
}

impl<T> FlexbuffersCodec<T> {
    pub(crate) fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T> Encoder for FlexbuffersCodec<T>
where
    T: Serialize,
{
    type Item<'a> = T;
    type Error = io::Error;

    fn encode(&mut self, item: Self::Item<'_>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = flexbuffers::to_vec(item).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Serialize error: {e}"))
        })?;
        dst.put_slice(&bytes);
        Ok(())
    }
}

impl<T> Decoder for FlexbuffersCodec<T>
where
    T: for<'de> Deserialize<'de>,
{
    type Item = T;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }
        let reader = flexbuffers::Reader::get_root(src.as_ref()).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Reader error: {e}"))
        })?;
        let item: T = Deserialize::deserialize(reader).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Deserialize error: {e}"),
            )
        })?;
        src.clear();
        Ok(Some(item))
    }
}
