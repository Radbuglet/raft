pub mod handshake {
    use crate::net::primitives::{codec_struct, ByteStr, VarInt};

    codec_struct! {
        #[derive(Debug, Clone)]
        pub struct SbHandshake {
            pub version: VarInt,
            pub server_addr: ByteStr,
            pub port: u16,
            pub next_state: VarInt,
        }
    }
}
