/// The hard maximum on the size of either a server-bound or client-bound packet.
///
/// This seems to be an additional artificial restriction on packet length.
///
/// [See wiki.vg for details.](https://wiki.vg/index.php?title=Protocol&oldid=18305#Packet_format).
pub const HARD_MAX_PACKET_LEN_INCL: u32 = 2 << 21 - 1;
