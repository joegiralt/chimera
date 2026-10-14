//! Bank 2 as the boot judged it (ADR 0137): `status`'s line, ABOUT's label.

use core::fmt::{self, Write};

/// Bank 1's state of the door, for the board.
pub use chimera_engine::bank2::{DOOR_CELL, DOOR_MAGIC, PROBE_ADDS, TABLE_LEN};
use chimera_image::Verdict;
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
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self, Self::Open { .. })
    }

    /// What ABOUT shows: no bank jargon (owner decision 8).
    pub fn label(&self) -> &'static str {
        if self.is_open() { "OK" } else { "NEEDS UPDATE" }
    }
}
