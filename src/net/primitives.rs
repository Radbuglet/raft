use bytes::{BufMut, Bytes};
use std::{any::type_name, mem, ops::Deref};

use crate::util::{
    bits::StaticBitSet,
    byte_cursor::{ByteReadCursor, Snip},
};

// === Traits === //

pub type StreamingDecodeResult<T> = anyhow::Result<Option<T>>;

pub trait StreamingCodec: SizedCodec {
    fn decode_streaming(cursor: &mut ByteReadCursor) -> StreamingDecodeResult<Self>;

    fn encode_streaming(&self, cursor: &mut impl BufMut);
}

pub trait SizedCodec: Codec {
    fn size(&self) -> usize;
}

pub trait Codec: Sized {
    fn decode(src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self>;

    fn encode(&self, cursor: &mut impl BufMut);

    fn decode_bytes(bytes: &Bytes) -> anyhow::Result<Self> {
        Self::decode(bytes, &mut ByteReadCursor::new(bytes))
    }
}

pub fn size_of_tiny<const MAX_SIZE: usize>(body: &impl StreamingCodec) -> usize {
    let mut container = &mut [0u8; MAX_SIZE][..];

    // `BufMut` is `&mut &mut [u8]`.
    body.encode(&mut container);

    // This works because `&mut [u8]`'s impl of `BufMut` advances the buffer by replacing the
    // slice reference with a shortened one.
    MAX_SIZE - container.len()
}

impl<T: StreamingCodec> Codec for T {
    fn decode(_snip: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        match Self::decode_streaming(cursor)? {
            Some(value) => Ok(value),
            None => anyhow::bail!(
                "incomplete primitive of type {} (location: {})",
                type_name::<Self>(),
                cursor.format_location(),
            ),
        }
    }

    fn encode(&self, cursor: &mut impl BufMut) {
        self.encode_streaming(cursor);
    }
}

// === Macros === //

#[doc(hidden)]
pub mod codec_struct_internals {
    pub use {
        super::{Codec, SizedCodec},
        crate::util::byte_cursor::{ByteReadCursor, Snip},
        anyhow::Result,
        bytes::BufMut,
        log::trace,
        std::{any::type_name, result::Result::Ok, stringify},
    };
}

macro_rules! codec_struct {
    ($(
		$(#[$attr:meta])*
		$struct_vis:vis struct $struct_name:ident {
			$($field_vis:vis $field_name:ident: $field_ty:ty),*
			$(,)?
		}
	)*) => {$(
		$(#[$attr])*
		$struct_vis struct $struct_name {
			$($field_vis $field_name: $field_ty,)*
		}

        impl $crate::net::primitives::codec_struct_internals::Codec for $struct_name {
			#[allow(unused_variables)]
            fn decode(
                src: &impl $crate::net::primitives::codec_struct_internals::Snip,
                cursor: &mut $crate::net::primitives::codec_struct_internals::ByteReadCursor,
            ) -> $crate::net::primitives::codec_struct_internals::Result<Self> {
				log::trace!(
					"Decoding {}...",
					$crate::net::primitives::codec_struct_internals::type_name::<Self>(),
				);
				$(
					let start_offset = cursor.read_count();
					let $field_name = $crate::net::primitives::codec_struct_internals::Codec::decode(src, cursor)?;
					$crate::net::primitives::codec_struct_internals::trace!(
						"\tDecoded {}: {:?} (ending offset: {}..{})",
						$crate::net::primitives::codec_struct_internals::stringify!($field_name),
						$field_name,
						start_offset,
						cursor.read_count(),
					);
				)*
				$crate::net::primitives::codec_struct_internals::Ok(Self { $($field_name,)* })
            }

			#[allow(unused_variables)]
			fn encode(
                &self,
                cursor: &mut impl $crate::net::primitives::codec_struct_internals::BufMut,
            ) {
				$($crate::net::primitives::codec_struct_internals::Codec::encode(&self.$field_name, cursor);)*
            }
        }

		impl $crate::net::primitives::codec_struct_internals::SizedCodec for $struct_name {
            fn size(&self) -> usize {
				$($crate::net::primitives::codec_struct_internals::SizedCodec::size(&self.$field_name) + )* 0
			}
        }
    )*};
}

pub(crate) use codec_struct;

// === Streaming Primitives === //

impl StreamingCodec for bool {
    fn decode_streaming(cursor: &mut ByteReadCursor) -> StreamingDecodeResult<Self> {
        let bytes = u8::decode_streaming(cursor)?;

        match bytes {
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            Some(got) => anyhow::bail!(
                "invalid variant for boolean; got: {got} (location: {})",
                cursor.format_location(),
            ),
            None => Ok(None),
        }
    }

    fn encode_streaming(&self, cursor: &mut impl BufMut) {
        cursor.put_u8(match *self {
            true => 1,
            false => 0,
        });
    }
}

impl SizedCodec for bool {
    fn size(&self) -> usize {
        1
    }
}

macro_rules! impl_prim {
    ($($ty:ty),*$(,)?) => {$(
		impl StreamingCodec for $ty {
			fn decode_streaming(cursor: &mut ByteReadCursor) -> StreamingDecodeResult<Self> {
				if let Some(bytes) = cursor.read_arr() {
					Ok(Some(<$ty>::from_be_bytes(bytes)))
				} else {
					Ok(None)
				}
			}

			fn encode_streaming(&self, cursor: &mut impl BufMut) {
				cursor.put_slice(&self.to_be_bytes());
			}
		}

		impl SizedCodec for $ty {
			fn size(&self) -> usize {
				mem::size_of::<Self>()
			}
		}
	)*};
}

impl_prim!(i8, u8, i16, u16, i32, u32, i64, f32, f64);

#[derive(Debug, Copy, Clone)]
pub struct VarInt(pub u32);

impl StreamingCodec for VarInt {
    fn decode_streaming(cursor: &mut ByteReadCursor) -> StreamingDecodeResult<Self> {
        let mut accum = 0u32;
        let mut shift = 0;

        for i in 1..=5 {
            let Some(byte) = cursor.read() else { return Ok(None) };

            // Push the byte's 7 first bits into the number. Since this number is little-endian, this
            // means that we're reading from least significant bits to most significant bits.
            let Some(new_accum) = accum.checked_add(((byte & !u8::MSB) as u32) << shift) else {
				anyhow::bail!(
					"VarInt was malformed and overflew the accumulator (location: {}).",
					cursor.format_location(),
				);
			};
            accum = new_accum;
            shift += 7;

            // If the byte's most-significant bit is unset, we know we're done.
            if byte & u8::MSB == 0 {
                break;
            } else if i == 5 {
                // We've reached the end of our VarInt and yet, it claims that it has one more byte.
                anyhow::bail!(
                    "VarInt was malformed as it claims to have one more byte in an already max-sized \
					 byte sequence (location: {}).",
					cursor.format_location(),
                );
            }
        }

        Ok(Some(Self(accum)))
    }

    fn encode_streaming(&self, cursor: &mut impl BufMut) {
        let mut accum = self.0;

        loop {
            let byte = accum & !u8::MSB as u32;
            accum >>= 7;

            if accum > 0 {
                cursor.put_u8(byte as u8 | u8::MSB);
            } else {
                cursor.put_u8(byte as u8);
                break;
            }
        }
    }
}

impl SizedCodec for VarInt {
    fn size(&self) -> usize {
        size_of_tiny::<5>(self)
    }
}

// === Codec === //

impl Codec for Bytes {
    fn decode(src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        let bytes = src.freeze_range(cursor.remaining());
        cursor.advance_remaining();
        Ok(bytes)
    }

    fn encode(&self, cursor: &mut impl BufMut) {
        cursor.put_slice(self);
    }
}

impl SizedCodec for Bytes {
    fn size(&self) -> usize {
        self.len()
    }
}

#[derive(Debug, Clone, Default)]
pub struct ByteString(Bytes);

impl ByteString {
    pub fn from_string(str: String) -> Self {
        Self(Bytes::from(str.into_bytes()))
    }

    pub fn from_static_str(str: &'static str) -> Self {
        Self(Bytes::from_static(str.as_bytes()))
    }

    pub fn from_bytes(bytes: Bytes) -> Result<Self, std::str::Utf8Error> {
        let _ = std::str::from_utf8(&bytes)?;
        Ok(Self(bytes))
    }

    pub unsafe fn from_bytes_unchecked(bytes: Bytes) -> Self {
        Self(bytes)
    }

    pub fn bytes(&self) -> &Bytes {
        &self.0
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

impl Deref for ByteString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        unsafe {
            // Safety: by structure invariants, we know that this byte buffers's contents are always
            // valid UTF-8.
            std::str::from_utf8_unchecked(&self.0)
        }
    }
}

impl Codec for ByteString {
    fn decode(snip: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        let size = VarInt::decode(snip, cursor)?.0;

        let Some(data) = cursor.read_slice(size as usize) else {
			anyhow::bail!(
				"Packet did not contain the necessary bytes to form the string: remaining: {}, \
				 expected: {} (location: {}).",
				 cursor.remaining().len(),
				 size,
				 cursor.format_location(),
			);
		};

        match Self::from_bytes(snip.freeze_range(data)) {
            Ok(str) => Ok(str),
            Err(err) => Err(anyhow::anyhow!(err).context(format!(
                "String byte data was not valid UTF8 (location: {}).",
                cursor.format_location(),
            ))),
        }
    }

    fn encode(&self, cursor: &mut impl BufMut) {
        const TOO_LONG: &str = "string is too long to send over the network";

        // Send string length in bytes.
        VarInt(self.0.len().try_into().expect(TOO_LONG)).encode(cursor);

        // Send string's UTF-8 encoded contents.
        cursor.put_slice(self.as_bytes());
    }
}

impl SizedCodec for ByteString {
    fn size(&self) -> usize {
        VarInt(self.0.len() as u32).size() + self.bytes().size()
    }
}

impl<T: Codec> Codec for Option<T> {
    fn decode(src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        Ok(if bool::decode(src, cursor)? {
            Some(T::decode(src, cursor)?)
        } else {
            None
        })
    }

    fn encode(&self, cursor: &mut impl BufMut) {
        if let Some(inner) = self {
            true.encode(cursor);
            inner.encode(cursor);
        } else {
            false.encode(cursor);
        }
    }
}

impl<T: SizedCodec> SizedCodec for Option<T> {
    fn size(&self) -> usize {
        if let Some(inner) = self {
            true.size() + inner.size()
        } else {
            false.size()
        }
    }
}
