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

const PINRSTF: u32 = 1 << 22;
const BORRSTF: u32 = 1 << 21;
const PORRSTF: u32 = 1 << 23;
const SFTRSTF: u32 = 1 << 24;
const IWDG1RSTF: u32 = 1 << 26;

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
