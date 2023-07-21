use super::primitives::{codec_struct, ByteString, Codec, VarInt};
use super::transport::FramedPacket;

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

			#[allow(dead_code)]
			impl Packet {
				pub fn decode(packet: &FramedPacket) -> anyhow::Result<Self> {
					match packet.id {
						$($id => Ok($packet_name::decode_bytes(&packet.body)?.into()),)*
						_ => anyhow::bail!(
							"Unrecognized packet ID ({}) for state {}",
							packet.id,
							type_name::<Self>(),
						),
					}
				}

				pub fn frame(self) -> FramedPacket<Self> {
					let id = match &self {
						$(Self::$packet_name(_) => $id,)*
					};

					FramedPacket { id, body: self }
				}
			}

			pub use Packet::*;

			// TODO: Implement CodecWrite for Packet.

			$(
				impl From<$packet_name> for Packet {
					fn from(packet: $packet_name) -> Self {
						Self::$packet_name(packet)
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
