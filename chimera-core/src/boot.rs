//! What a reset leads to: the synth, or ST's ROM DFU loader (USB console
//! spec § Enter DFU from the firmware). Pure, no I/O: the chip shell reads
//! and clears the marker, then acts on `after_reset`.

use core::fmt;

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

/// Who wrote the last marker, kept in RTC_BKP2R: "MENU" and "CONS".
pub const FROM_MENU: u32 = u32::from_be_bytes(*b"MENU");
pub const FROM_CONSOLE: u32 = u32::from_be_bytes(*b"CONS");

/// What the shell saw at the top of `main`, for `status`'s `boot` line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootSeen {
    /// RTC_BKP0R on entry.
    pub marker: u32,
    /// RTC_BKP0R after the clear.
    pub readback: u32,
    pub action: BootAction,
    /// RCC_RSR on entry, before RMVF.
    pub rsr: u32,
    /// PWR_CR1.DBP on entry, before the shell set it.
    pub dbp: bool,
    /// RTC_BKP1R, counted up every boot: it counts on through a power-off
    /// only if the backup domain kept power.
    pub boots: u32,
    /// RTC_BKP2R: `FROM_MENU`, `FROM_CONSOLE`, or 0 if nothing wrote one.
    pub from: u32,
    /// RTC_BKP3R: RCC_RSR at the last jump to the ROM loader, 0 if none.
    pub jump_rsr: u32,
}

impl fmt::Display for BootSeen {
    /// `boot marker=… readback=… action=Synth rsr=… dbp=0 boots=… from=… jump_rsr=…`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let action = match self.action {
            BootAction::Synth => "Synth",
            BootAction::RomDfu => "RomDfu",
        };
        write!(
            f,
            "boot marker={:08x} readback={:08x} action={action} rsr={:08x} dbp={} boots={} from=",
            self.marker,
            self.readback,
            self.rsr,
            u8::from(self.dbp),
            self.boots,
        )?;
        match self.from {
            0 => f.write_str("none")?,
            FROM_MENU => f.write_str("menu")?,
            FROM_CONSOLE => f.write_str("console")?,
            x => write!(f, "{x:08x}")?,
        }
        write!(f, " jump_rsr={:08x}", self.jump_rsr)
    }
}

/// What the DFU prompt's yes confirms. `Witnessed<Witness = ()>`: nothing
/// on the card is at stake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RomDfu;
