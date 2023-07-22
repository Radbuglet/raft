use super::primitives::{codec_struct, ByteArray, Codec, NetString, SizedCodec, Uuid, VarInt};
use super::transport::{FramedPacket, UnframedPacket};

use crate::util::byte_cursor::{ByteReadCursor, Snip};

use bytes::{BufMut, Bytes};
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
				$($field_name:ident: $field_ty:ty $(=> $field_config:expr)?),*
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

			impl Codec<()> for Packet {
				#[allow(unused_variables)]
				fn decode(_args: (), src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
					let id = VarInt::decode((), src, cursor)?.0;

					match id {
						$($id => Ok($packet_name::decode((), src, cursor)?.into()),)*
						_ => anyhow::bail!("Unknown packet with ID {id} in state {}", type_name::<Self>()),
					}
				}

				#[allow(unused_variables)]
			    fn encode(&self, _args: (), cursor: &mut impl BufMut) {
					#[allow(unreachable_patterns)]
					match self {
						$(Self::$packet_name(packet) => {
							VarInt($id).encode((), cursor);
							packet.encode((), cursor);
						})*
						_ => unreachable!(),
					}
				}
			}

			impl SizedCodec<()> for Packet {
				fn size(&self, _args: ()) -> usize {
					#[allow(unreachable_patterns)]
					match self {
						$(Self::$packet_name(packet) => VarInt($id).size(()) + packet.size(()),)*
						_ => unreachable!(),
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
					$(pub $field_name: $field_ty $(=> $field_config)?,)*
				}
			)*}
		}
	)*};
}

// === Packet Protocol === //

derive_protocol! {
    // === Handshake === //

    pub mod sb_handshake {
        #[derive(Debug, Clone)]
        struct Handshake(0) {
            version: VarInt,
            server_addr: NetString => 255,
            port: u16,
            next_state: VarInt,
        }
    }

    // === Status === //

    pub mod cb_status {
        #[derive(Debug, Clone)]
        struct StatusResponse(0) {
            json_resp: NetString,
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

    // === Login === //

    pub mod cb_login {
        #[derive(Debug, Clone)]
        struct Disconnect(0) {
            reason: NetString,
        }

        #[derive(Debug, Clone)]
        struct EncryptionRequest(1) {
            server_id: NetString => 20,
            public_key: ByteArray,
            verify_token: ByteArray,
        }

        // TODO
    }

    pub mod sb_login {
        #[derive(Debug, Clone)]
        struct LoginStart(0) {
            name: NetString => 16,
            player_uuid: Option<Uuid>,
        }

        #[derive(Debug, Clone)]
        struct EncryptionResponse(1) {
            shared_secret: ByteArray,
            verify_token: ByteArray,
        }

        #[derive(Debug, Clone)]
        struct LoginPluginResponse(2) {
            message_id: VarInt,
            data: Option<Bytes>,
        }
    }
}
