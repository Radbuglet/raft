use std::{fmt, io::Write, mem, str};

use anyhow::Context;
use bytes::Bytes;

use crate::util::{
    proto::{
        byte_stream::{ByteCursor, ByteWriteStream, WriteCodepointCounter},
        core::Codec,
        decode_seq::{
            DeserializeSeq, DeserializeSeqFor, DeserializeSeqForSimple, EndPosSummary,
            SeqDecodeCodec,
        },
        encode::{EncodeCodec, SerializeInto, WriteStreamFor},
    },
    var_int::{decode_var_i32_streaming, encode_var_u32},
};

pub struct MineCodec {
    _never: (),
}

impl Codec for MineCodec {}

impl SeqDecodeCodec for MineCodec {
    type Reader<'a> = ByteCursor<'a>;
    type ReaderPos = usize;

    fn covariant_cast<'a: 'b, 'b>(reader: Self::Reader<'a>) -> Self::Reader<'b> {
        reader
    }
}

impl EncodeCodec for MineCodec {
    type WriteElement<'a> = [u8];
}

// === Numerics === //

// Primitives
macro_rules! impl_numerics {
	($($ty:ty),*$(,)?) => {$(
		impl DeserializeSeq<MineCodec> for $ty {
			type Summary = ();
			type View<'a> = Self;
		}

		impl DeserializeSeqForSimple<MineCodec, ()> for $ty {
			fn decode_simple<'a>(
				_bind: [&'a (); 0],
				cursor: &mut ByteCursor<'a>,
				_args: &mut (),
			) -> anyhow::Result<Self::View<'a>> {
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
			fn serialize(&self, stream: &mut impl WriteStreamFor<MineCodec>, _args: &mut ()) -> anyhow::Result<()> {
				stream.push(&self.to_be_bytes())?;
				Ok(())
			}
		}
	)*};
}

impl_numerics!(i8, u8, i16, u16, i32, u32, i64, f32, f64, u128);

impl DeserializeSeq<MineCodec> for bool {
    type Summary = ();
    type View<'a> = Self;
}

impl DeserializeSeqForSimple<MineCodec, ()> for bool {
    fn decode_simple<'a>(
        _bind: [&'a (); 0],
        cursor: &mut ByteCursor<'a>,
        _args: &mut (),
    ) -> anyhow::Result<Self::View<'a>> {
        let byte = u8::decode_simple([], cursor, &mut ())?;
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
    fn serialize(
        &self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        _args: &mut (),
    ) -> anyhow::Result<()> {
        SerializeInto::<MineCodec, u8, ()>::serialize(&(*self as u8), stream, &mut ())
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

impl DeserializeSeq<MineCodec> for VarInt {
    type Summary = EndPosSummary<usize>;
    type View<'a> = i32;
}

impl DeserializeSeqForSimple<MineCodec, ()> for VarInt {
    fn decode_simple<'a>(
        _bind: [&'a (); 0],
        cursor: &mut ByteCursor<'a>,
        _args: &mut (),
    ) -> anyhow::Result<Self::View<'a>> {
        decode_var_i32_streaming(cursor)?.ok_or_else(|| {
            anyhow::anyhow!("Invalid unterminated VarInt (location: {})", cursor.pos())
        })
    }
}

impl SerializeInto<MineCodec, VarInt, ()> for VarInt {
    fn serialize(
        &self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        _args: &mut (),
    ) -> anyhow::Result<()> {
        encode_var_u32(&mut stream.as_write(), self.0)?;
        Ok(())
    }
}

impl SerializeInto<MineCodec, VarInt, ()> for i32 {
    fn serialize(
        &self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        args: &mut (),
    ) -> anyhow::Result<()> {
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

impl DeserializeSeq<MineCodec> for VarUint {
    type Summary = EndPosSummary<usize>;
    type View<'a> = u32;
}

impl DeserializeSeqForSimple<MineCodec, ()> for VarUint {
    fn decode_simple<'a>(
        _bind: [&'a (); 0],
        cursor: &mut ByteCursor<'a>,
        _args: &mut (),
    ) -> anyhow::Result<Self::View<'a>> {
        let value = VarInt::decode_simple([], cursor, &mut ())?;
        u32::try_from(value).map_err(|_| {
            anyhow::anyhow!(
                "Unexpected negative VarUint with value {value} (location: {}).",
                cursor.format_location()
            )
        })
    }
}

impl SerializeInto<MineCodec, VarUint, ()> for VarUint {
    fn serialize(
        &self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        args: &mut (),
    ) -> anyhow::Result<()> {
        let value = i32::try_from(self.0).context("Attempted to send oversized VarUint")?;
        VarInt(value).serialize(stream, args)
    }
}

impl SerializeInto<MineCodec, VarUint, ()> for u32 {
    fn serialize(
        &self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        args: &mut (),
    ) -> anyhow::Result<()> {
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

impl DeserializeSeq<MineCodec> for TrailingByteArray {
    type Summary = ();
    type View<'a> = &'a [u8];
}

impl DeserializeSeqForSimple<MineCodec, ()> for TrailingByteArray {
    fn decode_simple<'a>(
        _bind: [&'a (); 0],
        cursor: &mut ByteCursor<'a>,
        _args: &mut (),
    ) -> anyhow::Result<Self::View<'a>> {
        let remaining = cursor.remaining();
        cursor.advance_remaining();
        Ok(remaining)
    }
}

impl SerializeInto<MineCodec, TrailingByteArray, ()> for TrailingByteArray {
    fn serialize(
        &self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        _args: &mut (),
    ) -> anyhow::Result<()> {
        stream.push(&self.0)?;
        Ok(())
    }
}

impl SerializeInto<MineCodec, TrailingByteArray, ()> for &'_ [u8] {
    fn serialize(
        &self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        _args: &mut (),
    ) -> anyhow::Result<()> {
        stream.push(self)?;
        Ok(())
    }
}

// String
impl DeserializeSeq<MineCodec> for String {
    type Summary = usize;
    type View<'a> = &'a str;
}

impl DeserializeSeqFor<MineCodec, Option<u32>> for String {
    fn summarize(
        cursor: &mut ByteCursor,
        max_len: &mut Option<u32>,
    ) -> anyhow::Result<Self::Summary> {
        let start_pos = cursor.pos();
        let size = VarUint::decode_simple([], cursor, &mut ())?;

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
        let mut codepoints = WriteCodepointCounter::default();
        codepoints.write_all(data)?;
        let codepoints = codepoints.codepoints().ok_or_else(|| {
            anyhow::anyhow!(
                "Failed to validate string ending at location {}",
                cursor.format_location()
            )
        })?;

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

    unsafe fn view<'a>(
        summary: &'a Self::Summary,
        cursor: ByteCursor<'a>,
        _args: &mut Option<u32>,
    ) -> Self::View<'a> {
        let mut cursor = cursor.with_pos(*summary);
        let size = VarUint::decode_simple([], &mut cursor, &mut ()).unwrap();
        let data = cursor.read_slice(size as usize).unwrap();

        // Safety: `summarize` already validated that parsing from the `*summary` buffer location
        // on its corresponding buffer (guaranteed by safety invariants) onwards will result in
        // a valid string being constructed, allowing us to skip the validation step.
        str::from_utf8_unchecked(data)
    }

    fn skip(summary: &Self::Summary, cursor: &mut ByteCursor, _args: &mut Option<u32>) {
        debug_assert_eq!(cursor.pos(), *summary);
        let byte_len = VarInt::decode_simple([], cursor, &mut ()).unwrap();
        let _ = cursor.read_slice(byte_len as usize);
    }
}

impl<T: fmt::Display> SerializeInto<MineCodec, String, Option<u32>> for T {
    fn serialize(
        &self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        args: &mut Option<u32>,
    ) -> anyhow::Result<()> {
        // Determine the size of the string.
        let mut counter = WriteCodepointCounter::default();
        write!(&mut counter, "{self}")?;

        // Validate length
        if let Some(max_len) = *args {
            let curr_len = counter.codepoints().unwrap();
            anyhow::ensure!(
				curr_len <= max_len as usize,
				"String {:?} has a max length of {max_len} codepoint(s) but was {curr_len} codepoint(s) long.",
				self.to_string(),
			);
        }

        // Write out the packet
        let len = i32::try_from(counter.bytes())
            .map_err(|_| anyhow::anyhow!("String max length overflew an i32."))?;

        VarInt(len).serialize(stream, &mut ())?;
        write!(stream.as_write(), "{self}")?;

        Ok(())
    }
}

impl DeserializeSeqFor<MineCodec, u32> for String {
    fn summarize(cursor: &mut ByteCursor, args: &mut u32) -> anyhow::Result<Self::Summary> {
        Self::summarize(cursor, &mut Some(*args))
    }

    unsafe fn view<'a>(
        summary: &'a Self::Summary,
        cursor: ByteCursor<'a>,
        args: &mut u32,
    ) -> Self::View<'a> {
        Self::view(summary, cursor, &mut Some(*args))
    }

    fn skip(summary: &Self::Summary, cursor: &mut ByteCursor, args: &mut u32) {
        Self::skip(summary, cursor, &mut Some(*args))
    }
}

impl<T: fmt::Display> SerializeInto<MineCodec, String, u32> for T {
    fn serialize(
        &self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        args: &mut u32,
    ) -> anyhow::Result<()> {
        SerializeInto::<MineCodec, String, Option<u32>>::serialize(self, stream, &mut Some(*args))
    }
}

// Identifier
#[derive(Debug, Clone)]
pub struct Identifier(pub String);

impl Identifier {
    pub const MAX_LEN: u32 = 32767;
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&'_ str> for Identifier {
    fn from(value: &'_ str) -> Self {
        Self(String::from(value))
    }
}

impl DeserializeSeq<MineCodec> for Identifier {
    type Summary = <String as DeserializeSeq<MineCodec>>::Summary;
    type View<'a> = &'a str;
}

impl DeserializeSeqFor<MineCodec, ()> for Identifier {
    fn summarize(cursor: &mut ByteCursor, _args: &mut ()) -> anyhow::Result<Self::Summary> {
        String::summarize(cursor, &mut Some(Self::MAX_LEN))
    }

    unsafe fn view<'a>(
        summary: &'a Self::Summary,
        cursor: ByteCursor<'a>,
        _args: &mut (),
    ) -> Self::View<'a> {
        String::view(summary, cursor, &mut Some(Self::MAX_LEN))
    }

    fn skip(summary: &Self::Summary, cursor: &mut ByteCursor, _args: &mut ()) {
        String::skip(summary, cursor, &mut Some(Self::MAX_LEN))
    }
}

impl<T: fmt::Display> SerializeInto<MineCodec, Identifier, ()> for T {
    fn serialize(
        &self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        _args: &mut (),
    ) -> anyhow::Result<()> {
        SerializeInto::<MineCodec, String, Option<u32>>::serialize(
            self,
            stream,
            &mut Some(Identifier::MAX_LEN),
        )
    }
}
