use std::io;

use super::{
    bits::{i32_from_u32_2c, i32_to_u32_2c, StaticBitSet},
    proto::byte_stream::ByteCursor,
};

pub fn decode_var_i32_streaming(cursor: &mut ByteCursor) -> anyhow::Result<Option<i32>> {
    let mut accum = 0u32;
    let mut shift = 0;

    loop {
        let Some(byte) = cursor.read() else { return Ok(None) };
        accum |= ((byte & !u8::MSB) as u32) << shift;

        if byte & u8::MSB == 0 {
            break;
        }

        shift += 7;

        if shift >= 32 {
            anyhow::bail!(
                "VarInt is too long to fit an i32 (location: {}).",
                cursor.format_location(),
            );
        }
    }

    let accum = i32_from_u32_2c(accum);
    Ok(Some(accum))
}

pub fn encode_var_u32(stream: &mut impl io::Write, value: i32) -> io::Result<()> {
    let mut accum = i32_to_u32_2c(value);

    loop {
        let byte = accum & !u8::MSB as u32;
        accum >>= 7;

        if accum > 0 {
            stream.write_all(&[byte as u8 | u8::MSB])?;
        } else {
            stream.write_all(&[byte as u8])?;
            break;
        }
    }

    Ok(())
}
