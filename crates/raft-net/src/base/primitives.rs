use std::{fmt, ops::Deref, str::Utf8Error};

use super::traits::{DecodeError, Serde};

use bytes::{Buf, BufMut, Bytes};

// === Integers === //

macro_rules! impl_int_serde {
    ($($ty:ty),* $(,)?) => {$(
        impl Serde for $ty {
            fn decode_cx(cursor: &mut impl Buf, _args: ()) -> Result<Self, DecodeError> {
                DecodeError::kinded(concat!("`", stringify!($ty), "`"), || {
                    Serde::decode(cursor).map(<$ty>::from_be_bytes)
                })
            }

            fn encode_cx(&self, cursor: &mut impl BufMut, _args: ()) {
                self.to_be_bytes().encode(cursor);
            }
        }
    )*};
}

impl_int_serde!(u8, i8, u16, i16, u32, i32, u64, i64, f32, f64);

impl Serde for bool {
    fn decode_cx(cursor: &mut impl Buf, _args: ()) -> Result<Self, DecodeError> {
        DecodeError::kinded("boolean", || match u8::decode(cursor)? {
            0x01 => Ok(true),
            0x00 => Ok(false),
            _ => Err(DecodeError::new_static(
                "byte was neither `0x01` nor `0x00`",
            )),
        })
    }

    fn encode_cx(&self, cursor: &mut impl BufMut, _args: ()) {
        match self {
            true => cursor.put_u8(0x01),
            false => cursor.put_u8(0x00),
        }
    }
}

// === VarInt === //

const SEGMENT_BITS: u8 = 0x7F;
const CONTINUE_BIT: u8 = 0x80;

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct VarInt(pub i32);

impl Serde for VarInt {
    fn decode_cx(cursor: &mut impl Buf, _args: ()) -> Result<Self, DecodeError> {
        DecodeError::kinded("VarInt", || {
            let mut value = 0u32;
            let mut position = 0;

            loop {
                let byte = u8::decode(cursor)?;
                value |= ((byte & SEGMENT_BITS) as u32) << position;

                if (byte & CONTINUE_BIT) == 0 {
                    break;
                }

                position += 7;

                if position >= 32 {
                    return Err(DecodeError::new_static("`VarInt` is too big"));
                }
            }

            Ok(VarInt(value as i32))
        })
    }

    fn encode_cx(&self, cursor: &mut impl BufMut, _args: ()) {
        let mut value = self.0 as u32;

        loop {
            if value & !(SEGMENT_BITS as u32) == 0 {
                (value as u8).encode(cursor);
            }

            (value as u8 & SEGMENT_BITS | CONTINUE_BIT).encode(cursor);

            value >>= 7;
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct VarLong(pub i64);

impl Serde for VarLong {
    fn decode_cx(cursor: &mut impl Buf, _args: ()) -> Result<Self, DecodeError> {
        DecodeError::kinded("VarLong", || {
            let mut value = 0u64;
            let mut position = 0;

            loop {
                let byte = u8::decode(cursor)?;
                value |= ((byte & SEGMENT_BITS) as u64) << position;

                if (byte & CONTINUE_BIT) == 0 {
                    break;
                }

                position += 7;

                if position >= 64 {
                    return Err(DecodeError::new_static("`VarLong` is too big"));
                }
            }

            Ok(VarLong(value as i64))
        })
    }

    fn encode_cx(&self, cursor: &mut impl BufMut, _args: ()) {
        let mut value = self.0 as u64;

        loop {
            if value & !(SEGMENT_BITS as u64) == 0 {
                (value as u8).encode(cursor);
            }

            (value as u8 & SEGMENT_BITS | CONTINUE_BIT).encode(cursor);

            value >>= 7;
        }
    }
}

// === String === //

#[derive(Clone, Hash, Eq, PartialEq)]
pub struct BufString(Bytes);

impl fmt::Debug for BufString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

impl fmt::Display for BufString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

impl BufString {
    pub fn try_new(data: Bytes) -> Result<Self, Utf8Error> {
        _ = std::str::from_utf8(&data)?;

        Ok(Self(data))
    }

    pub fn into_buf(self) -> Bytes {
        self.0
    }

    pub fn as_buf(&self) -> &Bytes {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
}

impl AsRef<str> for BufString {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Deref for BufString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Serde<usize> for BufString {
    fn decode_cx(cursor: &mut impl Buf, max_len: usize) -> Result<Self, DecodeError> {
        DecodeError::kinded("string", || {
            let VarInt(byte_len) = VarInt::decode(cursor)?;

            let Ok(byte_len) = usize::try_from(byte_len) else {
                return Err(DecodeError::new_static("length was negative"));
            };

            if byte_len > max_len * 3 {
                return Err(DecodeError::new_string(format!(
                    "UTF-8 length of {byte_len} necessarily goes beyond UTF-16 limit of {max_len}"
                )));
            }

            let utf_8 = Bytes::decode_cx(cursor, byte_len)?;
            let Ok(utf_8) = Self::try_new(utf_8) else {
                return Err(DecodeError::new_static("buffer contained invalid UTF-8"));
            };

            if utf16_codepoints(&utf_8, byte_len) > byte_len {
                return Err(DecodeError::new_string(format!(
                    "string buffer contains more than {max_len} UTF-16 codepoints"
                )));
            }

            Ok(utf_8)
        })
    }

    fn encode_cx(&self, cursor: &mut impl BufMut, _max_len: usize) {
        VarInt(self.len() as i32).encode(cursor);

        self.0.encode(cursor);
    }
}

fn utf16_codepoints(text: &str, stop_counting_after: usize) -> usize {
    let mut counter = 0;

    for ch in text.chars() {
        counter += 1;

        if ch as u32 > 0xFFFF {
            counter += 1
        }

        if counter > stop_counting_after {
            break;
        }
    }

    counter
}

// === NBT === //

// TODO: NBT

// === Remaining === //

// TODO: Text Component

// TODO: JSON Text Component

// TODO: Identifier

// TODO: Entity Metadata

// TODO: Slot

// TODO: Hashed Slot

// TODO: Position

// TODO: Angle

// TODO: UUID

// TODO: BitSet

// TODO: Fixed BitSet

// TODO: Optional

// TODO: Prefixed Optional

// TODO: Array

// TODO: Prefixed Array

// TODO: Enum

// TODO: EnumSet (n)

// TODO: Byte Array

// TODO: ID or X

// TODO: ID Set

// TODO: Sound Event

// TODO: Chat Type

// TODO: Teleport Flags

// TODO: Recipe Display

// TODO: Slot Display

// TODO: Chunk Data

// TODO: Light Data

// TODO: X or Y
