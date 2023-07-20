use bytes::BufMut;
use std::mem;
use thiserror::Error;

use crate::util::{bits::StaticBitSet, codec::ByteReadCursor};

#[derive(Debug, Clone, Error)]
#[error("packet primitive was malformed")]
pub struct PrimitiveMalformedError;

pub type PrimitiveDecodeResult<T> = Result<Option<T>, PrimitiveMalformedError>;

pub trait StreamPrimitive: Sized {
    const MAX_SIZE: usize;

    fn decode(bytes: &mut ByteReadCursor) -> PrimitiveDecodeResult<Self>;

    fn encode(&self, bytes: &mut impl BufMut);

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

impl StreamPrimitive for bool {
    const MAX_SIZE: usize = 1;

    fn decode(bytes: &mut ByteReadCursor) -> PrimitiveDecodeResult<Self> {
        let bytes = u8::decode(bytes)?;

        match bytes {
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            Some(_) => Err(PrimitiveMalformedError),
            None => Ok(None),
        }
    }

    fn encode(&self, bytes: &mut impl BufMut) {
        bytes.put_u8(match *self {
            true => 1,
            false => 0,
        });
    }
}

macro_rules! impl_prim {
    ($($ty:ty),*$(,)?) => {$(
		impl StreamPrimitive for $ty {
			const MAX_SIZE: usize = mem::size_of::<Self>();

			fn decode(bytes: &mut ByteReadCursor) -> PrimitiveDecodeResult<Self> {
				if let Some(bytes) = bytes.read_arr() {
					Ok(Some(<$ty>::from_be_bytes(bytes)))
				} else {
					Ok(None)
				}
			}

			fn encode(&self, bytes: &mut impl BufMut) {
				bytes.put_slice(&self.to_be_bytes());
			}
		}
	)*};
}

impl_prim!(i8, u8, i16, u16, i32, u32, i64, f32, f64);

#[derive(Debug, Copy, Clone)]
pub struct VarInt(pub u32);

impl StreamPrimitive for VarInt {
    const MAX_SIZE: usize = 5;

    fn decode(bytes: &mut ByteReadCursor) -> PrimitiveDecodeResult<Self> {
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

    fn encode(&self, bytes: &mut impl BufMut) {
        let mut accum = self.0;

        loop {
            let byte = accum & !u8::MSB as u32;
            accum >>= 7;

            if accum > 0 {
                bytes.put_u8(byte as u8 | u8::MSB);
            } else {
                bytes.put_u8(byte as u8);
                break;
            }
        }
    }
}
