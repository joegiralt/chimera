//! What a reset leads to: the synth, or ST's ROM DFU loader (USB console
//! spec § Enter DFU from the firmware). Pure, no I/O: the chip shell reads
//! and clears the marker, then acts on `after_reset`.

use crate::reset::ResetCause;

/// "DFU!". 0 and every other value boot the synth.
pub const DFU_MAGIC: u32 = 0x4446_5521;

/// ST AN2606, STM32H74x/75x: the system memory bootloader's vector table.
pub const ROM_DFU_BASE: u32 = 0x1FF0_9800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootAction {
    Synth,
    RomDfu,
}

/// `marker`, read after a reset; `readback`, the marker read again after
/// the shell cleared it; `cause`, RCC_RSR's reset. Only `DFU_MAGIC` that
/// cleared to 0, after a software reset, enters the ROM loader. A
/// power-on, brown-out, pin or watchdog reset always boots the synth, so a
/// marker that outlived a power-off (a backup domain on VBAT) can't trap a
/// cold boot in DFU. A read-back that isn't 0 is stuck (DBP never set, or
/// the write didn't land) and boots the synth too.
pub const fn after_reset(marker: u32, readback: u32, cause: ResetCause) -> BootAction {
    if marker == DFU_MAGIC && readback == 0 && matches!(cause, ResetCause::Software) {
        BootAction::RomDfu
    } else {
        BootAction::Synth
    }
}

/// What the DFU prompt's yes confirms. `Witnessed<Witness = ()>`: nothing
/// on the card is at stake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomDfu;
