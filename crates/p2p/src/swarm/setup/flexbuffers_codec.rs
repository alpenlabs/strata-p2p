use std::io;

use asynchronous_codec::{Decoder, Encoder};
use bytes::{Buf, BufMut, BytesMut};
use serde::{Deserialize, Serialize};

/// Maximum frame size for setup messages in bytes.
///
/// This prevents pre-authentication OOM attacks by rejecting oversized frames
/// before they are processed or buffered.
const MAX_FRAME_SIZE: usize = 1_024 * 1_024; // 1MB

/// A codec for flexbuffers-based encoding/decoding of messages.
#[derive(Clone, Debug, Default)]
pub(crate) struct FlexbuffersCodec<T> {
    _marker: std::marker::PhantomData<T>,
}

impl<T> FlexbuffersCodec<T> {
    pub(crate) const fn new() -> Self {
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
        dst.put_u32(bytes.len() as u32);
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
        if src.len() < 4 {
            return Ok(None);
        }

        let mut length_bytes = [0u8; 4];
        length_bytes.copy_from_slice(&src[..4]);
        let length = u32::from_be_bytes(length_bytes) as usize;

        // Security: Reject frames exceeding MAX_FRAME_SIZE before processing
        // to prevent pre-authentication OOM attacks
        if length > MAX_FRAME_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Frame size {length} exceeds maximum allowed size of {MAX_FRAME_SIZE} bytes",
                ),
            ));
        }

        if src.len() < 4 + length {
            return Ok(None);
        }

        src.advance(4);
        let data = &src[..length];

        let reader = flexbuffers::Reader::get_root(data).map_err(|e| {
            io::Error::new(io::ErrorKind::InvalidData, format!("Reader error: {e}"))
        })?;
        let item: T = Deserialize::deserialize(reader).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Deserialize error: {e}"),
            )
        })?;
        src.advance(length);
        Ok(Some(item))
    }
}
