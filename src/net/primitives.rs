use std::mem;
use thiserror::Error;

use crate::util::{bits::StaticBitSet, codec::ByteCursor};

#[derive(Debug, Clone, Error)]
#[error("packet primitive was malformed")]
pub struct PrimitiveMalformedError;

pub type PrimitiveDecodeResult<T> = Result<Option<T>, PrimitiveMalformedError>;

pub trait Primitive: Sized {
    const MAX_SIZE: usize;

    fn decode(bytes: &mut ByteCursor) -> PrimitiveDecodeResult<Self>;
}

impl Primitive for bool {
    const MAX_SIZE: usize = 1;

    fn decode(bytes: &mut ByteCursor) -> PrimitiveDecodeResult<Self> {
        let bytes = u8::decode(bytes)?;

        match bytes {
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            Some(_) => Err(PrimitiveMalformedError),
            None => Ok(None),
        }
    }
}

macro_rules! impl_prim {
    ($($ty:ty),*$(,)?) => {$(
		impl Primitive for $ty {
			const MAX_SIZE: usize = mem::size_of::<Self>();

			fn decode(bytes: &mut ByteCursor) -> PrimitiveDecodeResult<Self> {
				if let Some(bytes) = bytes.read_arr() {
					Ok(Some(<$ty>::from_be_bytes(bytes)))
				} else {
					Ok(None)
				}
			}
		}
	)*};
}

impl_prim!(i8, u8, i16, u16, i32, u32, i64, f32, f64);

#[derive(Debug, Copy, Clone)]
pub struct VarInt(pub u32);

impl Primitive for VarInt {
    const MAX_SIZE: usize = 5;

    fn decode(bytes: &mut ByteCursor) -> PrimitiveDecodeResult<Self> {
        let mut accum = 0u32;
        let mut shift = 0;

        for _ in 0..5 {
            let Some(byte) = bytes.read() else { return Ok(None) };

            // Push the byte's 7 first bits into the number. Since this number is little-endian, this
            // means that we're reading from least significant bits to most significant bits.
            accum += ((byte & !u8::MSB) as u32) << shift;
            shift += 7;

            // If the byte's most-significant bit is unset, we know we're done.
            if byte & u8::MSB == 0 {
                break;
            }
        }

        Ok(Some(Self(accum)))
    }
}
