//! What a reset leads to: the synth, or ST's ROM DFU loader (USB console
//! spec § Enter DFU from the firmware). Pure, no I/O: the chip shell reads
//! and clears the marker, then acts on `after_reset`.

/// "DFU!". 0, a power-on's garbage and every other value boot the synth.
pub const DFU_MAGIC: u32 = 0x4446_5521;

/// ST AN2606, STM32H74x/75x: the system memory bootloader's vector table.
pub const ROM_DFU_BASE: u32 = 0x1FF0_9800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootAction {
    Synth,
    RomDfu,
}

/// `marker`, read after a reset, and `readback`, the marker read again
/// after the shell cleared it. Only `DFU_MAGIC` that cleared to 0 enters
/// the ROM loader. A read-back that isn't 0 is stuck (DBP never set, or the
/// write didn't land) and boots the synth, so a stuck marker can't trap the
/// unit in DFU. A transient stuck leaves the magic in place, so a later
/// clean reset may still enter DFU: accepted, as the ROM loader can't brick
/// the unit and every boot tries the clear again.
pub const fn after_reset(marker: u32, readback: u32) -> BootAction {
    if marker == DFU_MAGIC && readback == 0 {
        BootAction::RomDfu
    } else {
        BootAction::Synth
    }
}

/// What the DFU prompt's yes confirms. `Witnessed<Witness = ()>`: nothing
/// on the card is at stake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomDfu;
