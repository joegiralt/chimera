//! Bank 2 as the boot judged it (ADR 0137): `status`'s line, ABOUT's label.

use core::fmt::{self, Write};

/// Bank 1's state of the door, for the board.
pub use chimera_engine::bank2::{
    BOARD_DOOR_ADDR, DOOR_CELL, DOOR_MAGIC, PROBE_ADDS, TABLE_LEN, UI_DOOR_ADDR,
};
use chimera_hal::store::Store;
use chimera_image::{FlashFail, Verdict};
pub use chimera_image::{Seal, Version};

use crate::dsp::tape_engine::Heads;
use crate::dsp::tape_engine::params::{TapeEngineParams, TapeModel};
use crate::dsp::tape_engine::read::SourceView;
use crate::project::{LoadKind, PartId, Project};
use crate::storage::{Ready, SourceHead};
use crate::ui::source_cell::{SourceCell, SourceJob};
use crate::ui::tape_strip::{Overview, Paint};

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

/// The text cell `addr` shows in place of its value under `bank2`: the
/// reverb's TYPE (ADR 0142) and TAPE's MODEL (ADR 0141) read `NEEDS
/// UPDATE` while their code can't play.
pub fn closed_text(
    addr: crate::addr::ParamAddr,
    bank2: Option<Bank2Status>,
) -> Option<&'static str> {
    use crate::addr::{BlockRef, ParamAddr};
    use crate::dsp::reverb::ReverbParams;
    use crate::dsp::tape_engine::params::TapeEngineParams;
    let named = [
        ParamAddr::new(BlockRef::Reverb, ReverbParams::TYPE),
        ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODEL),
    ];
    (closed(bank2) && named.contains(&addr)).then_some(NEEDS_UPDATE)
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

/// The UI's table in bank 2 (ADR 0175), at `UI_DOOR_ADDR`: TAPE's cells
/// and strip, which the engine's door can't name. Without `bank2`, and on
/// a host, the UI calls its own (`UiDoor::OWN`).
#[repr(C)]
pub struct UiDoor {
    magic: u32,
    pub(crate) overview: fn(&SourceView<'_>, f32, f32) -> Overview,
    pub(crate) strip_key: fn(&Overview, TapeModel, &TapeEngineParams, Heads) -> u32,
    #[allow(clippy::type_complexity)]
    pub(crate) strip:
        fn(&mut dyn Paint, &Overview, TapeModel, &TapeEngineParams, Heads, (i32, i32)),
    pub(crate) source_turn: fn(&mut SourceCell, &mut Project, PartId, i8),
    pub(crate) source_load: fn(&mut SourceCell, &mut Project, PartId),
    pub(crate) source_kind:
        fn(&SourceCell, &Project, PartId, u32) -> Option<(SourceHead, LoadKind)>,
    #[cfg(feature = "tape")]
    pub(crate) source_tick: fn(&mut SourceCell, &mut Project),
    pub(crate) source_run: fn(&mut SourceCell, SourceJob, &mut dyn Store, &Ready, &mut Project),
}

/// A card row's head and how it loads (`SourceCell::kind`).
#[cfg_attr(not(feature = "tape"), allow(dead_code))]
type HeadKind = (SourceHead, LoadKind);

/// `UiDoor::magic`.
const UI_MAGIC: u32 = 0xB2C0_0001;

impl UiDoor {
    /// Every entry is `tape_strip::raster`'s or `source_cell::card`'s, the
    /// modules `bank2.x` links into bank 2.
    pub const OWN: UiDoor = UiDoor {
        magic: UI_MAGIC,
        overview: Overview::of_in,
        strip_key: crate::ui::tape_strip::raster::key_in,
        strip: crate::ui::tape_strip::raster::strip_in,
        source_turn: SourceCell::turn_in,
        source_load: SourceCell::load_in,
        source_kind: SourceCell::kind_in,
        #[cfg(feature = "tape")]
        source_tick: SourceCell::rehead_in,
        source_run: SourceCell::run_in,
    };
}

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.door.ui")]
#[used]
static UI_DOOR: UiDoor = UiDoor::OWN;

/// `f` on the UI's table: bank 2's through the door on the unit with
/// `bank2`, `None` closed; this build's own elsewhere; `None` without
/// `tape`.
#[inline(always)]
pub fn ui_door<R>(f: impl FnOnce(&UiDoor) -> R) -> Option<R> {
    #[cfg(all(feature = "bank2", target_os = "none"))]
    {
        // SAFETY: `bank2.x` links `UI_DOOR`, a `UiDoor`, at its slot,
        // `UI_DOOR_ADDR` (its assert).
        unsafe { DOOR_CELL.with_at(UI_DOOR_ADDR, f) }
    }
    #[cfg(all(feature = "tape", not(all(feature = "bank2", target_os = "none"))))]
    {
        Some(f(&UiDoor::OWN))
    }
    // Without TAPE's DSP (the bench) TAPE's UI is as closed, and links out.
    #[cfg(not(feature = "tape"))]
    {
        let _ = f;
        None
    }
}

/// TAPE's UI can't run: bank 2 closed. A build without `tape` plays TAPE
/// silent and unlabelled (ADR 0141), so never there.
pub fn ui_closed() -> bool {
    cfg!(feature = "tape") && ui_door(|_| ()).is_none()
}

/// The UI's table answers (the boot's door check).
pub fn ui_door_ok() -> bool {
    ui_door(|d| d.magic == UI_MAGIC) == Some(true)
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
