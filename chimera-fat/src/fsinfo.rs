//! FAT32's FSInfo sector: advisory, so checked and never trusted.

use crate::blocks::BLOCK;
use crate::volume::Layout;

const LEAD: (usize, u32) = (0, 0x4161_5252);
const STRUCT: (usize, u32) = (484, 0x6141_7272);
const TRAIL: (usize, u32) = (508, 0xAA55_0000);
const NEXT_FREE: usize = 492;

fn u32_at(b: &[u8; BLOCK], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Its three signatures match.
pub fn valid(b: &[u8; BLOCK]) -> bool {
    [LEAD, STRUCT, TRAIL]
        .iter()
        .all(|&(at, sig)| u32_at(b, at) == sig)
}

/// The next-free hint, only from a valid FSInfo and only a data cluster.
pub fn hint(b: &[u8; BLOCK], l: &Layout) -> Option<u32> {
    let c = u32_at(b, NEXT_FREE);
    (valid(b) && l.holds(c)).then_some(c)
}
