//! Bank 2 as the boot judged it (ADR 0137): `status`'s line, ABOUT's label.

use core::fmt::{self, Write};

/// Bank 1's state of the door, for the board.
pub use chimera_engine::bank2::{
    BENCH_DOOR_ADDR, BOARD_DOOR_ADDR, Checked, DOOR_CELL, DOOR_MAGIC, DoorSlot, Judged, PROBE_ADDS,
    TABLE_LEN, UI_DOOR_ADDR,
};
use chimera_hal::store::Store;
use chimera_image::{FlashFail, Verdict};
pub use chimera_image::{Seal, Version};

use crate::addr::ParamAddr;
use crate::block::{Block, ParamId};
use crate::dsp::tape_engine::params::{TapeEngineParams, TapeMachine};
use crate::dsp::tape_engine::read::SourceView;
use crate::dsp::tape_engine::{Heads, NeedsRam};
use crate::preset::Sound;
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

/// The text cell `addr` shows in place of its value `v` (0..1, as
/// displayed) under `bank2`: the reverb's TYPE (ADR 0142), TAPE's MODEL
/// (ADR 0141) and a VA MODE whose oscillators are bank 2's (FM, ADR 0163)
/// and the FX TYPE FUZZ (ADR 0157), the filter TYPE SEM (ADR 0192), VA
/// NOISE's KIND (ADR 0197, every KIND) and the delay TYPE GRAIN (ADR 0202,
/// which plays as TAPE) read `NEEDS UPDATE` while their code can't play.
pub fn closed_text(
    addr: crate::addr::ParamAddr,
    v: f32,
    bank2: Option<Bank2Status>,
) -> Option<&'static str> {
    use crate::addr::{BlockRef, ParamAddr};
    use crate::block::DiskCode;
    use crate::dsp::reverb::ReverbParams;
    use crate::dsp::tape_engine::params::TapeEngineParams;
    use crate::dsp::va::{VaMode, VaParams};
    use crate::dsp::voice_fx::params::{FxType, VoiceFxParams};
    let named = [
        ParamAddr::new(BlockRef::Reverb, ReverbParams::TYPE),
        ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODEL),
        ParamAddr::new(BlockRef::Va, VaParams::NOISE_KIND),
    ];
    let va_mode = || {
        addr == ParamAddr::new(BlockRef::Va, VaParams::MODE)
            && addr.spec().is_some_and(|s| {
                VaMode::from_disk_code(crate::ui::fmt::discrete(v, s.fmt.max_int()))
                    .is_some_and(VaMode::in_bank2)
            })
    };
    let fx_type = || {
        addr == ParamAddr::new(BlockRef::VoiceFx, VoiceFxParams::TYPE)
            && addr.spec().is_some_and(|s| {
                FxType::from_disk_code(crate::ui::fmt::discrete(v, s.fmt.max_int()))
                    .is_some_and(FxType::in_bank2)
            })
    };
    let kind = || {
        use crate::dsp::filter::FilterKind;
        use crate::params::FilterParams;
        addr == ParamAddr::new(BlockRef::Filter, FilterParams::KIND)
            && addr.spec().is_some_and(|s| {
                FilterKind::BUILT
                    .get(usize::from(crate::ui::fmt::discrete(v, s.fmt.max_int())))
                    .is_some_and(|k| k.in_bank2())
            })
    };
    let grain = || {
        use crate::dsp::delay::{DelayParams, DelayType};
        addr == ParamAddr::new(BlockRef::Delay, DelayParams::TYPE)
            && DelayType::at(v) == DelayType::Grain
    };
    (closed(bank2) && (named.contains(&addr) || va_mode() || fx_type() || kind() || grain()))
        .then_some(NEEDS_UPDATE)
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
/// a host, the UI calls its own (`UiDoor::OWN`). Its entries are called
/// through its methods, so none leaves `ui_door`:
///
/// ```compile_fail,E0616
/// let _ = chimera_core::bank2::ui_door(|d| d.strip);
/// ```
#[repr(C)]
pub struct UiDoor {
    magic: u32,
    overview: fn(&SourceView<'_>, f32, f32) -> Overview,
    strip_key: fn(&Overview, TapeMachine, &TapeEngineParams, Heads) -> u32,
    #[allow(clippy::type_complexity)]
    strip: fn(&mut dyn Paint, &Overview, TapeMachine, &TapeEngineParams, Heads, (i32, i32)),
    source_turn: fn(&mut SourceCell, &mut Project, PartId, i8),
    source_load: fn(&mut SourceCell, &mut Project, PartId),
    source_kind: fn(&SourceCell, &Project, PartId, crate::storage::Stem) -> Option<HeadKind>,
    #[cfg(feature = "tape")]
    source_tick: fn(&mut SourceCell, &mut Project),
    source_run: fn(&mut SourceCell, SourceJob, &mut dyn Store, &Ready, &mut Project),
    /// START's and END's onset snap (`ui::onset`, ADR 0185).
    onset: fn(&[u8], u32, u32, bool) -> Option<u32>,
    /// WALK's machine defaults, cell notes and held refusals (`ui::walk`,
    /// ADR 0203).
    machine_edit: fn(ParamAddr, f32, &mut dyn Block) -> EnvWrites,
    tape_note: fn(ParamAddr, &Sound) -> Option<&'static str>,
    band_note: fn(ParamAddr, &Sound, crate::ui::view::SourceState) -> Option<&'static str>,
    tape_held: fn(&TapeEngineParams, ParamId, u8) -> Option<NeedsRam>,
    /// TAPE cells' units where the engine has a law (`ui::walk`).
    tape_fmt: fn(
        ParamId,
        TapeMachine,
        Option<crate::dsp::tape_engine::Tron>,
    ) -> Option<crate::ui::page::ValFmt>,
    law_text: fn(&mut crate::ui::fmt::FmtBuf, crate::dsp::modulator::law::Law, f32),
    /// The delay's other TYPE's cells (`ui::walk`, ADR 0202).
    delay_note: fn(ParamId, crate::dsp::delay::DelayType) -> Option<&'static str>,
    /// A TAPE MODEL or DIRECTION edit's knob writes (`ui::walk`).
    tape_writes: fn(ParamId, u8, &TapeEngineParams) -> u8,
    /// The focus readout's full words for TAPE's MODEL and DIRECTION.
    readout_word: fn(ParamAddr, f32, Option<u8>) -> Option<&'static str>,
    /// SCRUB's reel a UI pass (`ui::jog`, ADR 0205).
    jog: fn(&mut crate::ui::jog::Jog, chimera_hal::Ms, f32) -> f32,
    /// PIT's tuner (`ui::pit_tuner`, ADR 0201): its pass and its paint.
    pit: fn(&mut crate::ui::UiState),
    pit_paint: fn(&mut dyn Paint, &crate::ui::pit_tuner::Shown),
}

/// A card row's head and how it loads (`SourceCell::kind`).
type HeadKind = (SourceHead, LoadKind);

/// The ENV 1 cells a MODEL or DIRECTION turn writes, each with its value.
type EnvWrites = &'static [(ParamId, f32)];

/// `UiDoor::magic`.
const UI_MAGIC: u32 = 0xB2C0_0007;

impl UiDoor {
    /// Every entry is `tape_strip::raster`'s, `source_cell::card`'s,
    /// `onset::detect`'s or `pit_tuner::calc`'s, the modules `bank2.x`
    /// links into bank 2.
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
        onset: crate::ui::onset::onset_in_bank2,
        machine_edit: crate::ui::walk::machine_edit_in,
        tape_note: crate::ui::walk::tape_note_in,
        band_note: crate::ui::walk::band_note_in,
        tape_held: crate::ui::walk::tape_held_in,
        tape_fmt: crate::ui::walk::tape_fmt_in,
        law_text: crate::ui::walk::law_text_in,
        delay_note: crate::ui::walk::delay_note_in,
        readout_word: crate::ui::walk::readout_word_in,
        tape_writes: crate::ui::walk::tape_writes_in,
        jog: crate::ui::jog::Jog::pass,
        pit: crate::ui::pit_tuner::calc::pass_in,
        pit_paint: crate::ui::pit_tuner::calc::paint_in,
    };

    #[inline(always)]
    pub(crate) fn overview(&self, v: &SourceView<'_>, start: f32, end: f32) -> Overview {
        (self.overview)(v, start, end)
    }

    #[inline(always)]
    pub(crate) fn strip_key(
        &self,
        o: &Overview,
        m: TapeMachine,
        p: &TapeEngineParams,
        heads: Heads,
    ) -> u32 {
        (self.strip_key)(o, m, p, heads)
    }

    #[inline(always)]
    pub(crate) fn strip(
        &self,
        on: &mut dyn Paint,
        o: &Overview,
        m: TapeMachine,
        p: &TapeEngineParams,
        heads: Heads,
        at: (i32, i32),
    ) {
        (self.strip)(on, o, m, p, heads, at)
    }

    #[inline(always)]
    pub(crate) fn source_turn(&self, c: &mut SourceCell, p: &mut Project, part: PartId, d: i8) {
        (self.source_turn)(c, p, part, d)
    }

    #[inline(always)]
    pub(crate) fn source_load(&self, c: &mut SourceCell, p: &mut Project, part: PartId) {
        (self.source_load)(c, p, part)
    }

    #[inline(always)]
    pub(crate) fn source_kind(
        &self,
        c: &SourceCell,
        p: &Project,
        part: PartId,
        stem: crate::storage::Stem,
    ) -> Option<HeadKind> {
        (self.source_kind)(c, p, part, stem)
    }

    #[cfg(feature = "tape")]
    #[inline(always)]
    pub(crate) fn source_tick(&self, c: &mut SourceCell, p: &mut Project) {
        (self.source_tick)(c, p)
    }

    #[inline(always)]
    pub(crate) fn source_run(
        &self,
        c: &mut SourceCell,
        job: SourceJob,
        s: &mut dyn Store,
        r: &Ready,
        p: &mut Project,
    ) {
        (self.source_run)(c, job, s, r, p)
    }

    #[inline(always)]
    pub(crate) fn pit(&self, ui: &mut crate::ui::UiState) {
        (self.pit)(ui)
    }

    #[inline(always)]
    pub(crate) fn pit_paint(&self, on: &mut dyn Paint, s: &crate::ui::pit_tuner::Shown) {
        (self.pit_paint)(on, s)
    }

    #[inline(always)]
    pub(crate) fn onset(&self, held: &[u8], frames: u32, from: u32, fwd: bool) -> Option<u32> {
        (self.onset)(held, frames, from, fwd)
    }

    #[inline(always)]
    pub(crate) fn machine_edit(&self, a: ParamAddr, before: f32, b: &mut dyn Block) -> EnvWrites {
        (self.machine_edit)(a, before, b)
    }

    #[inline(always)]
    pub(crate) fn tape_note(&self, a: ParamAddr, s: &Sound) -> Option<&'static str> {
        (self.tape_note)(a, s)
    }

    #[inline(always)]
    pub(crate) fn band_note(
        &self,
        a: ParamAddr,
        s: &Sound,
        src: crate::ui::view::SourceState,
    ) -> Option<&'static str> {
        (self.band_note)(a, s, src)
    }

    #[inline(always)]
    pub(crate) fn tape_held(
        &self,
        t: &TapeEngineParams,
        id: ParamId,
        code: u8,
    ) -> Option<NeedsRam> {
        (self.tape_held)(t, id, code)
    }

    #[inline(always)]
    pub(crate) fn tape_fmt(
        &self,
        id: ParamId,
        m: TapeMachine,
        once: Option<crate::dsp::tape_engine::Tron>,
    ) -> Option<crate::ui::page::ValFmt> {
        (self.tape_fmt)(id, m, once)
    }

    #[inline(always)]
    pub(crate) fn law_text(
        &self,
        buf: &mut crate::ui::fmt::FmtBuf,
        law: crate::dsp::modulator::law::Law,
        v: f32,
    ) {
        (self.law_text)(buf, law, v)
    }

    #[inline(always)]
    pub(crate) fn delay_note(
        &self,
        id: ParamId,
        k: crate::dsp::delay::DelayType,
    ) -> Option<&'static str> {
        (self.delay_note)(id, k)
    }

    #[inline(always)]
    pub(crate) fn readout_word(
        &self,
        a: ParamAddr,
        v: f32,
        held: Option<u8>,
    ) -> Option<&'static str> {
        (self.readout_word)(a, v, held)
    }

    #[inline(always)]
    pub(crate) fn tape_writes(&self, id: ParamId, code: u8, t: &TapeEngineParams) -> u8 {
        (self.tape_writes)(id, code, t)
    }

    #[inline(always)]
    pub(crate) fn jog(
        &self,
        j: &mut crate::ui::jog::Jog,
        now_ms: chimera_hal::Ms,
        drag: f32,
    ) -> f32 {
        (self.jog)(j, now_ms, drag)
    }
}

/// `UI_DOOR`'s slot.
#[cfg(feature = "bank2")]
// SAFETY: `bank2.x` links `UI_DOOR`, a `UiDoor`, at `UI_DOOR_ADDR` (its
// assert, `__bank2_ui_door`).
const UI_SLOT: DoorSlot<UiDoor> = unsafe { DoorSlot::new(UI_DOOR_ADDR) };

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
        DOOR_CELL.with_slot(UI_SLOT, f)
    }
    #[cfg(all(feature = "tape", not(all(feature = "bank2", target_os = "none"))))]
    {
        // A host test under `bank2` answers as the unit does: only while
        // the door cell is open (`open_host`).
        #[cfg(all(feature = "bank2", feature = "test-support"))]
        if !DOOR_CELL.is_open() {
            return None;
        }
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

/// The UI's table in judged door `j` answers (the boot's door check, run
/// before the door opens).
#[cfg(feature = "bank2")]
pub fn ui_door_ok(j: &chimera_engine::bank2::Judged) -> bool {
    j.with_slot(UI_SLOT, |d| d.magic == UI_MAGIC)
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
