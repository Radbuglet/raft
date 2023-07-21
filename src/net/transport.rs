use bytes::Bytes;
use futures::SinkExt;
use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, Encoder, Framed};

use crate::util::byte_cursor::{ByteMutReadSession, Snip};

use super::{
    limits::HARD_MAX_PACKET_LEN_INCL,
    primitives::{Codec, SizedCodec, StreamingCodec, VarInt},
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

    pub async fn read(&mut self) -> Option<anyhow::Result<Bytes>> {
        self.stream.next().await
    }

    pub async fn write(&mut self, packet: impl UnframedPacket) -> anyhow::Result<()> {
        self.stream.send(packet.frame()).await
    }

    pub fn set_max_recv_len(&mut self, len: u32) {
        self.stream.codec_mut().max_recv_len = len.min(HARD_MAX_PACKET_LEN_INCL);
    }
}

// === Packet traits === //

pub trait FramedPacket: SizedCodec {}

pub trait UnframedPacket {
    type Framed: FramedPacket;

    fn frame(self) -> Self::Framed;
}

// === Codecs === //

#[derive(Debug, Copy, Clone, Default)]
struct MinecraftCodec {
    pub max_recv_len: u32,
    pub is_compressed: bool,
}

impl Decoder for MinecraftCodec {
    type Item = Bytes;
    type Error = anyhow::Error;

    // TODO: Handle legacy framing of packets.
    fn decode(&mut self, stream: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        log::trace!("MinecraftCodec is buffering {} byte(s).", stream.len());

        let stream = ByteMutReadSession::new(stream);
        let cursor = &mut stream.cursor();

        if !self.is_compressed {
            // Decode length, validate it, and ensure we have the capacity to hold it.
            let Some(length) = VarInt::decode_streaming(cursor)? else { return Ok(None) };

            if length.0 > self.max_recv_len {
                anyhow::bail!(
					"received packet of {length:?} byte(s) while the codec was set to accept only {} byte(s)",
					self.max_recv_len,
				);
            }

            stream.reserve(length.0 as usize);

            // Decode the body
            let Some(body) = cursor.read_slice(length.0 as usize) else { return Ok(None) };

            // Construct a frame for it
            let body = stream.freeze_range(body);
            stream.consume_cursor(&cursor);

            Ok(Some(body))
        } else {
            todo!();
        }
    }
}

impl<B: FramedPacket> Encoder<B> for MinecraftCodec {
    type Error = anyhow::Error;

    fn encode(&mut self, packet: B, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        if !self.is_compressed {
            let size = packet.size();

            // Validate packet size
            let Some(size) = size
                .try_into()
                .ok()
                .filter(|&v| v < HARD_MAX_PACKET_LEN_INCL)
            else {
                anyhow::bail!("Attempted to send packet of size {size}, which is too big!");
            };

            // Write out packet
            VarInt(size).encode(dst);
            packet.encode(dst);

            Ok(())
        } else {
            todo!();
        }
    }
}
