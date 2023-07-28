use super::primitives::{
    codec_struct, ByteArray, Chat, Codec, Identifier, NetString, SizedCodec, Uuid, VarInt,
};
use super::transport::{FramedPacket, UnframedPacket};

use crate::util::{bytes_integration::Snip, proto::byte_stream::ByteCursor};

use bytes::{BufMut, Bytes};
use std::any::type_name;

// === Core === //

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
				fn decode(_args: (), src: &impl Snip, cursor: &mut ByteCursor) -> anyhow::Result<Self> {
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
				#[derive(Debug, Clone)]
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
        struct Handshake(0) {
            version: VarInt,
            server_addr: NetString => 255,
            port: u16,
            next_state: VarInt,
        }
    }

    // === Status === //

    pub mod cb_status {
        struct StatusResponse(0) {
            json_resp: NetString,
        }

        struct PingResponse(1) {
            payload: i64,
        }
    }

    pub mod sb_status {
        struct StatusRequest(0) {}

        struct PingRequest(1) {
            payload: i64,
        }
    }

    // === Login === //

    pub mod cb_login {
        struct Disconnect(0) {
            reason: Chat,
        }

        struct EncryptionRequest(1) {
            server_id: NetString => 20,
            public_key: ByteArray,
            verify_token: ByteArray,
        }

        struct LoginSuccess(2) {
            uuid: Uuid,
            username: NetString => 16,
            properties: Vec<structs::Property> => || {},
        }

        struct SetCompression(3) {
            threshold: VarInt,
        }

        struct LoadPluginRequest(4) {
            message_id: VarInt,
            channel: Identifier,
            data: Bytes,
        }
    }

    pub mod sb_login {
        struct LoginStart(0) {
            name: NetString => 16,
            player_uuid: Option<Uuid>,
        }

        struct EncryptionResponse(1) {
            shared_secret: ByteArray,
            verify_token: ByteArray,
        }

        struct LoginPluginResponse(2) {
            message_id: VarInt,
            data: Option<Bytes>,
        }
    }
}

// === Reusable Structures === //

pub mod structs {
    use super::*;

    codec_struct! {
        #[derive(Debug, Clone)]
        pub struct Property {
            name: NetString => 32767,
            value: NetString => 32767,
            signature: Option<NetString> => 32767,
        }
    }
}
