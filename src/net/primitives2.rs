use std::{io, mem, str};

use anyhow::Context;
use bytes::Bytes;

use crate::util::{
    byte_codec::{
        Codec, Deserialize, DeserializeFor, DeserializeForSimple, NoExternalCall, SerializeInto,
    },
    byte_cursor::ByteReadCursor,
    var_int::{decode_var_i32_streaming, encode_var_u32},
};

pub struct MineCodec {
    _never: (),
}

impl Codec for MineCodec {}

// === Numerics === //

// Primitives
macro_rules! impl_numerics {
	($($ty:ty),*$(,)?) => {$(
		impl Deserialize<MineCodec> for $ty {
			type Summary = ();
			type View<'a> = Self;
		}

		impl DeserializeForSimple<MineCodec, ()> for $ty {
			fn decode_simple<'a>(cursor: &mut ByteReadCursor<'a>, _args: &mut ()) -> anyhow::Result<Self> {
				let arr = cursor.read_arr()
					.ok_or_else(|| anyhow::anyhow!(
						"Unexpected end-of-stream while reading {}: expected {} byte(s), got {} \
						 (location: {}).",
						stringify!($ty),
						mem::size_of::<Self>(),
						cursor.remaining().len(),
						cursor.format_location(),
					))?;

				Ok(Self::from_be_bytes(arr))
			}
		}

		impl SerializeInto<MineCodec, $ty, ()> for $ty {
			fn serialize(&self, stream: &mut impl io::Write, _args: &mut ()) -> anyhow::Result<()> {
				stream.write_all(&self.to_be_bytes())?;
				Ok(())
			}
		}
	)*};
}

impl_numerics!(i8, u8, i16, u16, i32, u32, i64, f32, f64, u128);

impl Deserialize<MineCodec> for bool {
    type Summary = ();
    type View<'a> = Self;
}

impl DeserializeForSimple<MineCodec, ()> for bool {
    fn decode_simple<'a>(cursor: &mut ByteReadCursor<'a>, _args: &mut ()) -> anyhow::Result<Self> {
        let byte = u8::decode_simple(cursor, &mut ())?;

        match byte {
            0 => Ok(false),
            1 => Ok(true),
            _ => anyhow::bail!(
                "Invalid variant for boolean: expected 0 or 1, got {byte} (location: {})",
                cursor.format_location(),
            ),
        }
    }
}

impl SerializeInto<MineCodec, bool, ()> for bool {
    fn serialize(&self, stream: &mut impl io::Write, args: &mut ()) -> anyhow::Result<()> {
        (*self as u8).serialize(stream, args)
    }
}

// VarInt
#[derive(Debug, Copy, Clone)]
pub struct VarInt(pub i32);

impl From<i32> for VarInt {
    fn from(value: i32) -> Self {
        Self(value)
    }
}

impl Deserialize<MineCodec> for VarInt {
    type Summary = usize;
    type View<'a> = i32;
}

impl DeserializeForSimple<MineCodec, ()> for VarInt {
    fn decode_simple<'a>(
        cursor: &mut ByteReadCursor<'a>,
        _args: &mut (),
    ) -> anyhow::Result<Self::View<'a>> {
        decode_var_i32_streaming(cursor)?.ok_or_else(|| {
            anyhow::anyhow!("Invalid unterminated VarInt (location: {})", cursor.pos())
        })
    }
}

impl SerializeInto<MineCodec, VarInt, ()> for VarInt {
    fn serialize(&self, stream: &mut impl io::Write, _args: &mut ()) -> anyhow::Result<()> {
        encode_var_u32(stream, self.0)
    }
}

impl SerializeInto<MineCodec, VarInt, ()> for i32 {
    fn serialize(&self, stream: &mut impl io::Write, args: &mut ()) -> anyhow::Result<()> {
        VarInt(*self).serialize(stream, args)
    }
}

// VarUint
#[derive(Debug, Copy, Clone)]
pub struct VarUint(pub u32);

impl From<u32> for VarUint {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl Deserialize<MineCodec> for VarUint {
    type Summary = usize;
    type View<'a> = u32;
}

impl DeserializeForSimple<MineCodec, ()> for VarUint {
    fn decode_simple<'a>(
        cursor: &mut ByteReadCursor<'a>,
        _args: &mut (),
    ) -> anyhow::Result<Self::View<'a>> {
        let value = VarInt::decode_simple(cursor, &mut ())?;
        u32::try_from(value).map_err(|_| {
            anyhow::anyhow!(
                "Unexpected negative VarUint with value {value} (location: {}).",
                cursor.format_location()
            )
        })
    }
}

impl SerializeInto<MineCodec, VarUint, ()> for VarUint {
    fn serialize(&self, stream: &mut impl io::Write, args: &mut ()) -> anyhow::Result<()> {
        let value = i32::try_from(self.0).context("Attempted to send oversized VarUint")?;
        VarInt(value).serialize(stream, args)
    }
}

impl SerializeInto<MineCodec, VarUint, ()> for u32 {
    fn serialize(&self, stream: &mut impl io::Write, args: &mut ()) -> anyhow::Result<()> {
        VarUint(*self).serialize(stream, args)
    }
}

// === Containers === //

// TrailingByteArray
#[derive(Debug, Clone)]
pub struct TrailingByteArray(pub Bytes);

impl From<&'_ [u8]> for TrailingByteArray {
    fn from(value: &'_ [u8]) -> Self {
        Self(Bytes::from(Vec::from(value)))
    }
}

impl Deserialize<MineCodec> for TrailingByteArray {
    type Summary = ();
    type View<'a> = &'a [u8];
}

impl DeserializeForSimple<MineCodec, ()> for TrailingByteArray {
    fn decode_simple<'a>(
        cursor: &mut ByteReadCursor<'a>,
        _args: &mut (),
    ) -> anyhow::Result<Self::View<'a>> {
        let remaining = cursor.remaining();
        cursor.advance_remaining();
        Ok(remaining)
    }
}

impl SerializeInto<MineCodec, TrailingByteArray, ()> for TrailingByteArray {
    fn serialize(&self, stream: &mut impl io::Write, _args: &mut ()) -> anyhow::Result<()> {
        stream.write_all(&self.0)?;
        Ok(())
    }
}

impl SerializeInto<MineCodec, TrailingByteArray, ()> for &'_ [u8] {
    fn serialize(&self, stream: &mut impl io::Write, _args: &mut ()) -> anyhow::Result<()> {
        stream.write_all(self)?;
        Ok(())
    }
}

// String
impl Deserialize<MineCodec> for String {
    type Summary = usize;
    type View<'a> = &'a str;
}

impl DeserializeFor<MineCodec, Option<u32>> for String {
    fn summarize(
        cursor: &mut ByteReadCursor,
        max_len: &mut Option<u32>,
    ) -> anyhow::Result<Self::Summary> {
        let start_pos = cursor.pos();
        let size = VarUint::decode_simple(cursor, &mut ())?;

        // Validate length
        if let Some(max_len) = *max_len {
            let max_size = max_len
                .checked_mul(4)
                .filter(|&v| i32::try_from(v).is_ok())
                .unwrap_or_else(|| {
                    panic!(
						"Strings with a maximum codepoint length of {max_len} are untenable due to \
						 encoding constraints."
					)
                });

            anyhow::ensure!(
                size <= max_len * 4,
                "String byte stream is too long. The string is limited to {max_len} codepoint(s), \
				 which can be encoded in up to {max_size} bytes, but the size of the string in \
				 bytes is specified as {size} (location: {}).",
                cursor.format_location(),
            );
        }

        // Fetch bytes
        let data = cursor.read_slice(size as usize).ok_or_else(|| {
            anyhow::anyhow!(
                "Packet did not contain the necessary bytes to form the string. Available: {}, \
				 Expected: {} (location: {}).",
                cursor.remaining().len(),
                size,
                cursor.format_location(),
            )
        })?;

        // Validate bytes
        let codepoints = {
            let mut buffer = [0u8; 4];
            let mut buffer_offset = 0;
            let mut codepoints = 0;

            for &byte in data {
                buffer[buffer_offset] = byte;

                if std::str::from_utf8(&buffer[0..buffer_offset]).is_ok() {
                    buffer_offset = 0;
                    codepoints += 1;
                } else {
                    buffer_offset += 1;
                    anyhow::ensure!(
                        buffer_offset < 4,
                        "Failed to validate string ending at location {}",
                        cursor.format_location()
                    );
                }
            }

            codepoints
        };

        if let Some(max_len) = *max_len {
            if codepoints > max_len as usize {
                anyhow::bail!(
                    "String is too long: can contain at most {max_len} codepoint(s) but \
					 contains {codepoints} (location: {}).",
                    cursor.format_location(),
                );
            }
        }

        Ok(start_pos)
    }

    fn view_<'a>(
        _no_external_call: NoExternalCall,
        summary: &'a Self::Summary,
        cursor: ByteReadCursor<'a>,
        _args: &mut Option<u32>,
    ) -> Self::View<'a> {
        let mut cursor = cursor.with_offset(*summary);
        let size = VarUint::decode_simple(&mut cursor, &mut ()).unwrap();
        let data = cursor.read_slice(size as usize).unwrap();

        unsafe {
            // Safety: `summarize` already validated that parsing from the `*summary` buffer location
            // on its corresponding buffer (guaranteed by safety invariants) onwards will result in
            // a valid string being constructed, allowing us to skip the validation step.
            str::from_utf8_unchecked(data)
        }
    }

    fn end(summary: &Self::Summary, cursor: ByteReadCursor, _args: &mut Option<u32>) -> usize {
        let mut cursor = cursor.with_offset(*summary);
        let byte_len = VarInt::decode_simple(&mut cursor, &mut ()).unwrap();
        let _ = cursor.read_slice(byte_len as usize);
        cursor.pos()
    }
}

// TODO: Serialization
