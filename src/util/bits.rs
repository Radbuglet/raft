pub trait StaticBitSet {
    const MSB: Self;
}

impl StaticBitSet for u8 {
    const MSB: Self = 0x80;
}
