use bytes::Bytes;
use futures::SinkExt;
use tokio::net::TcpStream;
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, Encoder, Framed};

use crate::{
    net::primitives::VarUint,
    util::bytes_integration::{ByteMutReadSession, Snip},
};

use super::primitives::{Codec, SizedCodec, StreamingCodec};

// === Streams === //

/// The hard maximum on the size of either a server-bound or client-bound packet.
///
/// This seems to be an additional artificial restriction on packet length.
///
/// [See wiki.vg for details.](https://wiki.vg/index.php?title=Protocol&oldid=18305#Packet_format).
pub const HARD_MAX_PACKET_LEN_INCL: u32 = 2 << 21 - 1;

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
                    compression_threshold: None,
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

pub trait FramedPacket: SizedCodec<()> {}

pub trait UnframedPacket {
    type Framed: FramedPacket;

    fn frame(self) -> Self::Framed;
}

// === Codecs === //

#[derive(Debug, Copy, Clone, Default)]
struct MinecraftCodec {
    pub max_recv_len: u32,
    pub compression_threshold: Option<u32>,
}

impl Decoder for MinecraftCodec {
    type Item = Bytes;
    type Error = anyhow::Error;

    // TODO: Handle legacy framing of packets.
    fn decode(&mut self, stream: &mut bytes::BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        log::trace!("MinecraftCodec is buffering {} byte(s).", stream.len());

        let stream = ByteMutReadSession::new(stream);
        let cursor = &mut stream.cursor();

        if let Some(_compression_threshold) = self.compression_threshold {
            todo!();
        } else {
            // Decode length, validate it, and ensure we have the capacity to hold it.
            let Some(length) = VarUint::decode_streaming(cursor)? else { return Ok(None) };

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
        }
    }
}

impl<B: FramedPacket> Encoder<B> for MinecraftCodec {
    type Error = anyhow::Error;

    fn encode(&mut self, packet: B, dst: &mut bytes::BytesMut) -> Result<(), Self::Error> {
        if let Some(_compression_threshold) = self.compression_threshold {
            todo!();
        } else {
            let size = packet.size(());

            // Validate packet size
            let Some(size) = size
                .try_into()
                .ok()
                .filter(|&v| v < HARD_MAX_PACKET_LEN_INCL)
            else {
                anyhow::bail!("Attempted to send packet of size {size}, which is too big!");
            };

            // Write out packet
            VarUint(size).encode((), dst);
            packet.encode((), dst);

            Ok(())
        }
    }
}
