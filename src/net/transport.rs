use bytes::{BufMut, Bytes};
use futures::SinkExt;
use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, Encoder, Framed};

use crate::util::codec::{ByteMutReadSession, Snip};

use super::{
    limits::HARD_MAX_PACKET_LEN_INCL,
    primitives::{TinyCodec, VarInt},
};

// === Streams === //

#[derive(Debug)]
pub struct RawPeerStream {
    stream: Framed<TcpStream, MinecraftCodec>,
}

impl RawPeerStream {
    pub fn new(stream: TcpStream, max_recv_len: u32) -> Self {
        Self {
            stream: Framed::new(
                stream,
                MinecraftCodec {
                    max_recv_len: max_recv_len.min(HARD_MAX_PACKET_LEN_INCL),
                    is_compressed: false,
                },
            ),
        }
    }

    pub async fn read(&mut self) -> Option<anyhow::Result<RawPacket>> {
        self.stream.next().await
    }

    pub async fn write(&mut self, packet: RawPacket) -> anyhow::Result<()> {
        self.stream.send(packet).await
    }

    pub fn set_max_recv_len(&mut self, len: u32) {
        self.stream.codec_mut().max_recv_len = len.min(HARD_MAX_PACKET_LEN_INCL);
    }
}

#[derive(Debug, Clone)]
pub struct RawPacket {
    pub id: u32,
    pub body: Bytes,
}

// === Codecs === //

#[derive(Debug, Copy, Clone, Default)]
struct MinecraftCodec {
    pub max_recv_len: u32,
    pub is_compressed: bool,
}

impl Decoder for MinecraftCodec {
    type Item = RawPacket;
    type Error = anyhow::Error;

    // TODO: Handle legacy framing of packets.
    fn decode(&mut self, stream: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        log::trace!("MinecraftCodec is buffering {} byte(s).", stream.len());

        let stream = ByteMutReadSession::new(stream);
        let cursor = &mut stream.cursor();

        if !self.is_compressed {
            // Decode length, validate it, and ensure we have the capacity to hold it.
            let Some(length) = VarInt::decode_tiny(cursor)? else { return Ok(None) };

            if length.0 > self.max_recv_len {
                anyhow::bail!(
					"received packet of {length:?} byte(s) while the codec was set to accept only {} byte(s)",
					self.max_recv_len,
				);
            }

            stream.reserve(length.0 as usize);

            // Decode the packet ID; this may cause us to parse more than the allotted length but we
            // check for that scenario later so this is fine.
            let id_pos = cursor.read_count();
            let Some(id) = VarInt::decode_tiny(cursor)? else { return Ok(None) };

            // Decode the body
            let body_pos = cursor.read_count();
            let Some(body_len) = length.0.checked_sub((body_pos - id_pos) as u32) else {
				anyhow::bail!("received packet with a negative body size");
			};
            let Some(body) = cursor.read_slice(body_len as usize) else { return Ok(None) };

            // Construct a frame for it
            let body = stream.freeze_range(body);
            stream.consume_cursor(&cursor);

            Ok(Some(RawPacket { id: id.0, body }))
        } else {
            todo!();
        }
    }
}

impl Encoder<RawPacket> for MinecraftCodec {
    type Error = anyhow::Error;

    fn encode(&mut self, packet: RawPacket, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        if !self.is_compressed {
            // Determine the length of the packet ID.
            let id_len = VarInt(packet.id).length::<{ VarInt::MAX_SIZE }>();

            // Determine the overall packet len.
            let packet_len = id_len + packet.body.len();
            let Some(packet_len) = u32::try_from(packet_len)
				.ok().filter(|&v| v <= HARD_MAX_PACKET_LEN_INCL)
			else {
				anyhow::bail!("packet is too big (length: {})", packet_len);
			};

            // Write out the packet.
            VarInt(packet_len).encode_tiny(dst);
            VarInt(packet.id).encode_tiny(dst);
            dst.put(packet.body);

            Ok(())
        } else {
            todo!();
        }
    }
}
