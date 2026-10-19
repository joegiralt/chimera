//! Bank 2 as the boot judged it (ADR 0137): `status`'s line, ABOUT's label.

use core::fmt::{self, Write};

/// Bank 1's state of the door, for the board.
pub use chimera_engine::bank2::{DOOR_CELL, DOOR_MAGIC, PROBE_ADDS, TABLE_LEN};
use chimera_image::{FlashFail, Verdict};
pub use chimera_image::{Seal, Version};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bank2Status {
    /// This build was never sealed: it never opens bank 2.
    Unsealed,
    Empty,
    Torn,
    Foreign,
    Format(u16),
    /// Bad bounds or CRC, a read the flash refused, or a door that failed
    /// its check.
    Corrupt,
    /// Sound, but sealed for another build.
    Stale {
        have: Version,
    },
    Open {
        version: Version,
        seal: [u8; 8],
    },
    /// A double ECC error marked bank 2 bad and S0's erase failed: never
    /// read this boot.
    EccEraseFailed(FlashFail),
}

impl Bank2Status {
    /// `Good` is `Open`: the caller reports it only once the door opened.
    pub fn from_verdict(v: &Verdict) -> Self {
        match *v {
            Verdict::Empty => Self::Empty,
            Verdict::Torn => Self::Torn,
            Verdict::Foreign => Self::Foreign,
            Verdict::Format(n) => Self::Format(n),
            Verdict::Corrupt | Verdict::Unreadable => Self::Corrupt,
            Verdict::Stale { have } => Self::Stale { have },
            Verdict::Good(h) => Self::Open {
                version: h.version,
                seal: h.seal,
            },
        }
    }

    /// `bank2 open 0.2.0 02e440e2 seal=0123456789abcdef`, `bank2 empty`, …
    pub fn line(&self, w: &mut impl Write) -> fmt::Result {
        w.write_str("bank2 ")?;
        self.state(w)
    }

    /// The line without `bank2 `: `empty`, `stale have 0.2.0`, …
    pub fn state(&self, w: &mut impl Write) -> fmt::Result {
        match self {
            Self::Unsealed => w.write_str("unsealed"),
            Self::Empty => w.write_str("empty"),
            Self::Torn => w.write_str("torn"),
            Self::Foreign => w.write_str("foreign"),
            Self::Format(n) => write!(w, "format {n}"),
            Self::Corrupt => w.write_str("corrupt"),
            Self::Stale { have } => write!(w, "stale have {}", have.as_str()),
            Self::Open { version, seal } => {
                write!(w, "open {} seal=", version.as_str())?;
                seal.iter().try_for_each(|b| write!(w, "{b:02x}"))
            }
            Self::EccEraseFailed(f) => {
                w.write_str("ecc erase failed ")?;
                write_flash_fail(*f, w)
            }
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self, Self::Open { .. })
    }

    /// What ABOUT shows: no bank jargon (owner decision 8).
    pub fn label(&self) -> &'static str {
        if self.is_open() { "OK" } else { NEEDS_UPDATE }
    }
}

/// What a cell whose code is in bank 2 reads while bank 2 isn't open: the
/// focus band (and ABOUT) whole, the cell, a column wide, its last word.
pub const NEEDS_UPDATE: &str = "NEEDS UPDATE";
pub const NEEDS_UPDATE_CELL: &str = "UPDATE";

/// Whether bank 2's code can't play. `None` is a build without bank 2
/// (the desktop, the bench sets), which plays it all from bank 1.
fn closed(bank2: Option<Bank2Status>) -> bool {
    bank2.is_some_and(|s| !s.is_open())
}

/// The text cell `addr` shows in place of its value `v` (0..1, as
/// displayed) under `bank2`: the reverb's TYPE (ADR 0142), TAPE's MODEL
/// (ADR 0141) and a VA MODE whose oscillators are bank 2's (FM, ADR 0163)
/// read `NEEDS UPDATE` while their code can't play.
pub fn closed_text(
    addr: crate::addr::ParamAddr,
    v: f32,
    bank2: Option<Bank2Status>,
) -> Option<&'static str> {
    use crate::addr::{BlockRef, ParamAddr};
    use crate::block::DiskCode;
    use crate::dsp::reverb::ReverbParams;
    use crate::dsp::tape_engine::params::TapeEngineParams;
    use crate::dsp::va::{MODE_NAMES, VaMode, VaParams};
    let named = [
        ParamAddr::new(BlockRef::Reverb, ReverbParams::TYPE),
        ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODEL),
    ];
    let va_mode = || {
        addr == ParamAddr::new(BlockRef::Va, VaParams::MODE)
            && VaMode::from_disk_code(crate::ui::fmt::discrete(v, MODE_NAMES.len() as u8 - 1))
                .is_some_and(VaMode::in_bank2)
    };
    (closed(bank2) && (named.contains(&addr) || va_mode())).then_some(NEEDS_UPDATE)
}

/// What the header of a Part playing `engine` warns under `bank2`: a TAPE
/// Part is silent while TAPE can't play (ADR 0141). Beside the context and
/// the page, the header has room for the last word only.
pub fn closed_part(
    engine: crate::params::EngineType,
    bank2: Option<Bank2Status>,
) -> Option<&'static str> {
    (closed(bank2) && engine == crate::params::EngineType::Tape).then_some(NEEDS_UPDATE_CELL)
}

/// A flash op's failure in words: `timeout`, `errors 00040000`, …
pub fn write_flash_fail(f: FlashFail, w: &mut impl Write) -> fmt::Result {
    match f {
        FlashFail::Busy => w.write_str("busy"),
        FlashFail::Locked => w.write_str("locked"),
        FlashFail::NotWritable => w.write_str("not writable"),
        FlashFail::Errors(e) => write!(w, "errors {e:08x}"),
        FlashFail::Timeout => w.write_str("timeout"),
        FlashFail::NotErased => w.write_str("not erased"),
        FlashFail::NoClock => w.write_str("no cycle counter"),
    }
}
