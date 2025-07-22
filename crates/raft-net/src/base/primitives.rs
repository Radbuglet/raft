use super::traits::{Decode, DecodeCursor, DecodeError, Encode, EncodeCursor};

// === Integers === //

macro_rules! impl_int_serde {
    ($($ty:ty),* $(,)?) => {$(
        impl<C: DecodeCursor> Decode<C> for $ty {
            fn decode(cursor: &mut C, _args: ()) -> Result<Self, DecodeError> {
                DecodeError::kinded(concat!("`", stringify!($ty), "`"), || {
                    cursor.read().map(<$ty>::from_be_bytes)
                })
            }
        }
    )*};
}

impl_int_serde!(u8, i8, u16, i16, u32, i32, i64, f32, f64);

impl<C: DecodeCursor> Decode<C> for bool {
    fn decode(cursor: &mut C, _args: ()) -> Result<Self, DecodeError> {
        DecodeError::kinded("boolean", || match u8::decode(cursor, ())? {
            0x01 => Ok(true),
            0x00 => Ok(false),
            _ => Err(DecodeError::new_static(
                "byte was neither `0x01` nor `0x00`",
            )),
        })
    }
}

impl<C: EncodeCursor> Encode<C> for bool {
    fn encode(&self, cursor: &mut C, _args: ()) {
        match self {
            true => cursor.write_slice(&[0x01]),
            false => cursor.write_slice(&[0x00]),
        }
    }
}

// === VarInt === //

const SEGMENT_BITS: u8 = 0x7F;
const CONTINUE_BIT: u8 = 0x80;

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct VarInt(pub i32);

impl<C: DecodeCursor> Decode<C> for VarInt {
    fn decode(cursor: &mut C, _args: ()) -> Result<Self, DecodeError> {
        DecodeError::kinded("VarInt", || {
            let mut value = 0u32;
            let mut position = 0;

            loop {
                let byte = u8::decode(cursor, ())?;
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
}

impl<C: EncodeCursor> Encode<C> for VarInt {
    fn encode(&self, cursor: &mut C, _args: ()) {
        let mut value = self.0 as u32;

        loop {
            if value & !(SEGMENT_BITS as u32) == 0 {
                cursor.write_slice(&[value as u8]);
            }

            cursor.write_slice(&[value as u8 & SEGMENT_BITS | CONTINUE_BIT]);

            value >>= 7;
        }
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct VarLong(pub i64);

impl<C: DecodeCursor> Decode<C> for VarLong {
    fn decode(cursor: &mut C, _args: ()) -> Result<Self, DecodeError> {
        DecodeError::kinded("VarLong", || {
            let mut value = 0u64;
            let mut position = 0;

            loop {
                let byte = u8::decode(cursor, ())?;
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
}

impl<C: EncodeCursor> Encode<C> for VarLong {
    fn encode(&self, cursor: &mut C, _args: ()) {
        let mut value = self.0 as u64;

        loop {
            if value & !(SEGMENT_BITS as u64) == 0 {
                cursor.write_slice(&[value as u8]);
            }

            cursor.write_slice(&[value as u8 & SEGMENT_BITS | CONTINUE_BIT]);

            value >>= 7;
        }
    }
}

// === String === //

// TODO: String

// TODO: Text Component

// TODO: JSON Text Component

// TODO: Identifier

// TODO: VarInt

// TODO: VarLong

// TODO: Entity Metadata

// TODO: Slot

// TODO: Hashed Slot

// TODO: NBT

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
