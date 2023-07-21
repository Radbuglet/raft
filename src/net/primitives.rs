use bytes::{BufMut, Bytes};
use std::{any::type_name, mem, ops::Deref};

use crate::util::{
    bits::StaticBitSet,
    codec::{ByteReadCursor, Snip},
};

// === TinyCodec === //

pub type TinyDecodeResult<T> = anyhow::Result<Option<T>>;

pub trait TinyCodec: Sized + Codec {
    const MAX_SIZE: usize;

    fn decode_tiny(cursor: &mut ByteReadCursor) -> TinyDecodeResult<Self>;

    fn encode_tiny(&self, cursor: &mut impl BufMut);

    fn length<const MAX_SIZE: usize>(&self) -> usize {
        assert_eq!(MAX_SIZE, Self::MAX_SIZE);

        let mut container = &mut [0u8; MAX_SIZE][..];

        // `BufMut` is `&mut &mut [u8]`.
        self.encode(&mut container);

        // This works because `&mut [u8]`'s impl of `BufMut` advances the buffer by replacing the
        // slice reference with a shortened one.
        Self::MAX_SIZE - container.len()
    }
}

impl TinyCodec for bool {
    const MAX_SIZE: usize = 1;

    fn decode_tiny(cursor: &mut ByteReadCursor) -> TinyDecodeResult<Self> {
        let bytes = u8::decode_tiny(cursor)?;

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

    fn encode_tiny(&self, cursor: &mut impl BufMut) {
        cursor.put_u8(match *self {
            true => 1,
            false => 0,
        });
    }
}

macro_rules! impl_prim {
    ($($ty:ty),*$(,)?) => {$(
		impl TinyCodec for $ty {
			const MAX_SIZE: usize = mem::size_of::<Self>();

			fn decode_tiny(cursor: &mut ByteReadCursor) -> TinyDecodeResult<Self> {
				if let Some(bytes) = cursor.read_arr() {
					Ok(Some(<$ty>::from_be_bytes(bytes)))
				} else {
					Ok(None)
				}
			}

			fn encode_tiny(&self, cursor: &mut impl BufMut) {
				cursor.put_slice(&self.to_be_bytes());
			}
		}
	)*};
}

impl_prim!(i8, u8, i16, u16, i32, u32, i64, f32, f64);

#[derive(Debug, Copy, Clone)]
pub struct VarInt(pub u32);

impl TinyCodec for VarInt {
    const MAX_SIZE: usize = 5;

    fn decode_tiny(cursor: &mut ByteReadCursor) -> TinyDecodeResult<Self> {
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

    fn encode_tiny(&self, cursor: &mut impl BufMut) {
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

// === Codec === //

pub trait Codec: Sized {
    fn decode(src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self>;

    fn encode(&self, cursor: &mut impl BufMut);
}

impl<T: TinyCodec> Codec for T {
    fn decode(_snip: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        match Self::decode_tiny(cursor)? {
            Some(value) => Ok(value),
            None => anyhow::bail!(
                "incomplete primitive of type {} (location: {})",
                type_name::<Self>(),
                cursor.format_location(),
            ),
        }
    }

    fn encode(&self, cursor: &mut impl BufMut) {
        self.encode_tiny(cursor);
    }
}

#[derive(Debug, Clone, Default)]
pub struct ByteStr(Bytes);

impl ByteStr {
    pub fn from_str(str: &str) -> Self {
        Self(Bytes::from(Vec::from(str.as_bytes())))
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

impl Deref for ByteStr {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        unsafe {
            // Safety: by structure invariants, we know that this byte buffers's contents are always
            // valid UTF-8.
            std::str::from_utf8_unchecked(&self.0)
        }
    }
}

impl Codec for ByteStr {
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

#[doc(hidden)]
pub mod codec_struct_internals {
    pub use {
        super::Codec,
        crate::util::codec::{ByteReadCursor, Snip},
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

			fn encode(
                &self,
                cursor: &mut impl $crate::net::primitives::codec_struct_internals::BufMut,
            ) {
				$($crate::net::primitives::codec_struct_internals::Codec::encode(&self.$field_name, cursor);)*
            }
        }
    )*};
}

pub(crate) use codec_struct;
