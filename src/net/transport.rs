use bytes::Bytes;
use tokio_util::codec::Decoder;

use crate::util::codec::ByteReadSession;

use super::primitives::{Primitive, VarInt};

#[derive(Debug, Copy, Clone, Default)]
struct MinecraftCodec {
    pub max_len_exclusive: usize,
    pub is_compressed: bool,
}

impl Decoder for MinecraftCodec {
    type Item = (u32, Bytes);
    type Error = anyhow::Error;

    fn decode(&mut self, stream: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let stream = ByteReadSession::new(stream);
        let cursor = &mut stream.cursor();

        if !self.is_compressed {
            // Decode length, validate it, and ensure we have the capacity to hold it
            let Some(length) = VarInt::decode(cursor)? else { return Ok(None) };

            if length.0 >= self.max_len_exclusive as u32 {
                anyhow::bail!(
					"received packet of {length:?} byte(s) while the codec was set to accept only {} byte(s)",
					self.max_len_exclusive,
				);
            }

            stream.reserve(length.0 as usize);

            // Decode the packet ID
            let Some(id) = VarInt::decode(cursor)? else { return Ok(None) };

            // Decode the body
            let Some(body) = cursor.read_slice(length.0 as usize) else { return Ok(None) };

            // Construct a frame for it
            let body = stream.freeze_range(body);
            stream.consume_cursor(&cursor);

            Ok(Some((id.0, body)))
        } else {
            todo!();
        }
    }
}
