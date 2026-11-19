// The tables at the head of bank 2's S4 groups (ADR 0283, 0284), the one
// source: `bank2` includes this file, and chimera-stm32's build.rs includes
// it too and writes `bank2.x`'s placement and asserts from these values.

/// Where the S4 groups' tables start: S4 after MIXER's reserve (`bank2`
/// asserts it is `Slot::Mixer.base() + slot::RESERVE`).
pub const S4_TABLES_ADDR: u32 = 0x0818_0400;
/// Each table's room: the next starts this far on (`bank2` asserts each
/// table fits).
pub const DOOR_TABLE_STRIDE: u32 = 0x10;
/// VA's solos' table (`dsp::va::osc::solo::SoloDoor`, ADR 0283): the
/// door's own table is full, and a slot after it would take S0-S3 under
/// ADR 0251's 8 KB; the head of the S4 groups.
pub const VA_SOLO_DOOR_ADDR: u32 = S4_TABLES_ADDR;
/// The Parts' EQ's table (`part_eq::door::EqDoor`, ADR 0284): the next.
pub const PART_EQ_DOOR_ADDR: u32 = VA_SOLO_DOOR_ADDR + DOOR_TABLE_STRIDE;
