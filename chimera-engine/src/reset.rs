//! Why the chip last reset, from RCC_RSR's flags (RM0433, RCC reset status
//! register). Pure: `chimera-stm32` reads the register at boot, clears it,
//! and the AUDIO page shows the cause.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetCause {
    /// IWDG1: the audio interrupt stalled, or a fault or DMA error halted
    /// the core (ADR 0034). A sustained overrun of the timeout (100 ms) or
    /// more, audio starving the controls tick, reads the same.
    Watchdog,
    Software,
    Brownout,
    PowerOn,
    /// The NRST pin alone, driven from outside. A debug probe's reset
    /// (SYSRESETREQ) reads `Software`.
    Pin,
    Unknown,
}

/// "SOFT", in a spare backup register just before a firmware `sys_reset`.
pub const SOFT_INTENT: u32 = u32::from_be_bytes(*b"SOFT");

const PINRSTF: u32 = 1 << 22;
const BORRSTF: u32 = 1 << 21;
const PORRSTF: u32 = 1 << 23;
const SFTRSTF: u32 = 1 << 24;
const IWDG1RSTF: u32 = 1 << 26;
/// The flags `from_rsr` reads. Behind stock 1.09 bit 18 is left set on
/// every boot (seen on the unit, 2026-10-09), so it alone says nothing.
const KNOWN: u32 = PINRSTF | BORRSTF | PORRSTF | SFTRSTF | IWDG1RSTF;

impl ResetCause {
    /// The most specific flag wins: every internal reset also drives NRST,
    /// so PINRSTF comes with all of them, and a power-on sets BORRSTF too.
    pub const fn from_rsr(rsr: u32) -> Self {
        if rsr & IWDG1RSTF != 0 {
            ResetCause::Watchdog
        } else if rsr & SFTRSTF != 0 {
            ResetCause::Software
        } else if rsr & PORRSTF != 0 {
            ResetCause::PowerOn
        } else if rsr & BORRSTF != 0 {
            ResetCause::Brownout
        } else if rsr & PINRSTF != 0 {
            ResetCause::Pin
        } else {
            ResetCause::Unknown
        }
    }

    /// The cause behind a bootloader that clears RCC_RSR (stock 1.09): a
    /// `rsr` with a known flag is read as it is. One with none is told from the
    /// backup domain, which has no VBAT: `boots_before` 0 (fresh) is a
    /// power-on; `intent` set by the firmware is software; a kept domain
    /// and no intent is `Watchdog`, which a HardFault reset through the
    /// IWDG reads as too (ADR 0034). A reset the firmware did not ask for
    /// but the bootloader hides (NRST pin) reads `Watchdog` too.
    pub const fn infer(rsr: u32, boots_before: u32, intent: u32) -> Self {
        if rsr & KNOWN != 0 {
            Self::from_rsr(rsr)
        } else if boots_before == 0 {
            ResetCause::PowerOn
        } else if intent == SOFT_INTENT {
            ResetCause::Software
        } else {
            ResetCause::Watchdog
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            ResetCause::Watchdog => "WDOG",
            ResetCause::Software => "SOFT",
            ResetCause::Brownout => "BROWN",
            ResetCause::PowerOn => "POWER",
            ResetCause::Pin => "PIN",
            ResetCause::Unknown => "?",
        }
    }
}
