use super::primitives::{codec_struct, ByteString, Codec, SizedCodec, VarInt};
use super::transport::{FramedPacket, UnframedPacket};

use crate::util::byte_cursor::{ByteReadCursor, Snip};

use bytes::BufMut;
use std::any::type_name;

// === Core === //

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub enum PeerState {
    Handshake,
    Status,
    Login,
    Play,
}

macro_rules! derive_protocol {
    ($(
		$(#[$wrapper_attr:meta])*
		$wrapper_vis:vis mod $wrapper_name:ident {$(
			$(#[$packet_attr:meta])*
			struct $packet_name:ident($id:literal) {
				$($field_name:ident: $field_ty:ty),*
				$(,)?
			}
		)*}
	)*) => {$(
		$(#[$wrapper_attr])*
		$wrapper_vis mod $wrapper_name {
			use super::*;

			$(#[$wrapper_attr])*
			#[derive(Debug, Clone)]
			pub enum Packet {
				$($packet_name($packet_name),)*
			}

			pub use Packet::*;

			impl Codec for Packet {
				#[allow(unused_variables)]
				fn decode(src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
					let id = VarInt::decode(src, cursor)?.0;

					match id {
						$($id => Ok($packet_name::decode(src, cursor)?.into()),)*
						_ => anyhow::bail!("Unknown packet with ID {id} in state {}", type_name::<Self>()),
					}
				}

				#[allow(unused_variables)]
			    fn encode(&self, cursor: &mut impl BufMut) {
					match self {
						$(Self::$packet_name(packet) => {
							VarInt($id).encode(cursor);
							packet.encode(cursor);
						})*
					}
				}
			}

			impl SizedCodec for Packet {
				fn size(&self) -> usize {
					match self {
						$(Self::$packet_name(packet) => VarInt($id).size() + packet.size(),)*
					}
				}
			}

			impl FramedPacket for Packet {}

			$(
				impl From<$packet_name> for Packet {
					fn from(packet: $packet_name) -> Self {
						Self::$packet_name(packet)
					}
				}

				impl UnframedPacket for $packet_name {
					type Framed = Packet;

					fn frame(self) -> Self::Framed {
						self.into()
					}
				}
			)*

			codec_struct! {$(
				$(#[$packet_attr])*
				pub struct $packet_name {
					$(pub $field_name: $field_ty,)*
				}
			)*}
		}
	)*};
}

// === Protocol === //

derive_protocol! {
    pub mod sb_handshake {
        #[derive(Debug, Clone)]
        struct Handshake(0) {
            version: VarInt,
            server_addr: ByteString,
            port: u16,
            next_state: VarInt,
        }
    }

    pub mod cb_status {
        #[derive(Debug, Clone)]
        struct StatusResponse(0) {
            json_resp: ByteString,
        }

        #[derive(Debug, Clone)]
        struct PingResponse(1) {
            payload: i64,
        }
    }

    pub mod sb_status {
        #[derive(Debug, Clone)]
        struct StatusRequest(0) {}

        #[derive(Debug, Clone)]
        struct PingRequest(1) {
            payload: i64,
        }
    }
}
