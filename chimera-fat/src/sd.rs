//! Is a card in the slot? Asked before an acquire, so an empty slot costs
//! a few CMD0s, not the acquire deadline (#186). Pure: the shell sends
//! `CMD0` with CS low and hands over the `NCR_BYTES` it clocks back.

/// GO_IDLE_STATE with its CRC, which the card checks before SPI mode.
pub const CMD0: [u8; 6] = [0x40, 0x00, 0x00, 0x00, 0x00, 0x95];

/// NCR: a card answers within 8 bytes of a command.
pub const NCR_BYTES: usize = 8;

/// CMD0 tries before the slot counts as empty.
pub const PRESENCE_TRIES: usize = 3;

/// R1, the first byte within NCR with bit 7 clear. None when nothing
/// answered: MISO is pulled up, so an empty slot reads all 0xFF. A busy
/// card holds DO low, which reads 0x00: an answer.
pub fn r1_within_ncr(bytes: &[u8]) -> Option<u8> {
    bytes
        .iter()
        .take(NCR_BYTES)
        .copied()
        .find(|b| b & 0x80 == 0)
}
