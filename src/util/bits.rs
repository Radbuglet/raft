pub trait StaticBitSet {
    const MSB: Self;
}

impl StaticBitSet for u8 {
    const MSB: Self = 0x80;
}

pub fn i32_from_u32_2c(v: u32) -> i32 {
    i32::from_ne_bytes(v.to_ne_bytes())
}

pub fn i32_to_u32_2c(v: i32) -> u32 {
    u32::from_ne_bytes(v.to_ne_bytes())
}
