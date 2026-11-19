//! `ed since <n>`: the editor pulls what changed. `<n>` is the number of
//! the last answer it got (`0`: nothing yet). The unit keeps, per section,
//! a copy of what it last reported, and the last answer's number (`Gen`);
//! if `<n>` is it, each section's live state is compared with its copy, at
//! f32 bits, and only what differs is answered, else everything
//! (ADR 0276, 0285, 0289). The answer:
//!
//! ```text
//! (an empty line, opening every answer holding a whole section)
//! part <n> <ENGINE>                       PART, whole
//! panel <name> <variant>
//! val <addr> <normalized> <stored> "<text>"
//! source "<text>"
//! routes <n>                              ROUTES, whole
//! route <SRC>:<DEST> <amount> "<text>"
//! unroute <SRC>:<DEST>                    (a diff's: the route gone)
//! mixer                                   MIXER, whole
//! panel REVERB <TYPE>
//! val PART3.LEVEL …
//! OK <m> [more]
//! ```
//!
//! A section answers whole after another Part or engine (PART), a change
//! in what its texts read beside the bits, or when it is owed. A whole
//! section opens with its header, and the editor drops what it held for
//! that section. Sections are measured first and taken in order while the
//! answer fits `ANSWER_BYTES_MAX`; one that doesn't is owed, and the
//! answer ends `OK <m> more`: the editor asks again at once. A section
//! past the bound by itself is `ERR ed since too large`, keeping nothing.
//!
//! `m` is the new number. A `val` is `ed get`'s own line (`value::reading`),
//! so the two never disagree. A `refused` line is a set the box refused
//! at its turn since the last answer (`edits::Refusals`, the newest four),
//! in its toast's words, at the end of PART; one refused at `ERR` time is
//! said in that answer alone. Only the followed Part's, and only while PART
//! is a diff: a whole PART says none, and every answer drops them. The
//! number moves before the answer is written, so an answer that never
//! reached the editor leaves it asking with a stale one: everything again.
//!
//! Runs only in the console's answer path, never in audio. The unit never
//! pushes: no sequence numbers, timers or hashes.

use core::fmt::{self, Write};
use core::mem::MaybeUninit;

use super::addr::{BusBlock, EdBlock, PadBlock, PartBlock, SoundBlock};
use super::describe::{
    PanelName, env_variant, filter_variant, fx_variant, lfo_variant, machine_variant,
    modal_variant, op_variant, reverb_variant, tape_variant,
};
use super::edits::Refusals;
use super::lit::lit;
use super::route::{self, Routes};
use super::schema::Schema;
use super::value::{block, engine_of, line, reading, source_text, text_ctx};
use super::{EdAddr, describe};
use crate::addr::{BlockRef, Op};
use crate::console::ANSWER_BYTES_MAX;
use crate::dsp::chorus::ChorusMode;
use crate::dsp::delay::DelayType;
use crate::dsp::drum::types::{Kit, PadId};
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::dsp::reverb::ReverbType;
use crate::project::PartId;
use crate::ui::UiState;
use crate::ui::cell_text::CellOwn;
use crate::ui::view::SlotCtx;
use chimera_engine::block::DiskCode;
use chimera_engine::fmt_buf::ByteCount;
use chimera_engine::params::EngineType;

/// The number of an answer: never 0 (that is the editor's "I have
/// nothing"), and under a billion so it fits the nine digits `ed since`
/// takes. Past the last it wraps to 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gen(u32);

impl Gen {
    const LAST: u32 = 999_999_999;
    pub const FIRST: Gen = Gen(1);

    /// `n` as a number: `None` for 0, which is no answer at all.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.new"))]
    pub const fn new(n: u32) -> Option<Gen> {
        if n == 0 || n > Gen::LAST {
            None
        } else {
            Some(Gen(n))
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.next"))]
    pub const fn next(self) -> Gen {
        if self.0 == Gen::LAST {
            Gen::FIRST
        } else {
            Gen(self.0 + 1)
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.get"))]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A value's f32 bits as last reported.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Bits(u32);

impl Bits {
    /// Never a stored value (an f32 NaN): a block the Sound lacks, so a
    /// block that appears is reported whatever it holds.
    const ABSENT: Bits = Bits(u32::MAX);
}

/// The values PART keeps: one for each param of every block the followed
/// engine's schema has (`reachable`), in `block_at` order, a block the
/// Sound lacks included (so a slot never moves). DRUM's twelve pads take
/// the most; a test says it holds every engine's.
pub const SLOTS: usize = 711;
/// The values MIXER keeps: each Part's mix and sends, then the buses.
pub const MIXER_SLOTS: usize = mixer_slots();
/// `Described`'s words: the most slots a schema has.
const MASK_WORDS: usize = SLOTS.div_ceil(32);

/// The mixer's slots, counted from its blocks' tables.
const fn mixer_slots() -> usize {
    let parts = PartId::ALL.len()
        * (BlockRef::Part.specs().len() + chimera_engine::params::OutParams::SENDS.len());
    let mut n = parts;
    let mut i = 0;
    while i < BusBlock::ALL.len() {
        n += BusBlock::ALL[i].block().specs().len();
        i += 1;
    }
    n
}

const _: () = assert!(MIXER_SLOTS <= SLOTS);

/// Block `i` of schema `s`'s slots. An engine's: the Sound's in
/// `BlockRef::ALL` order, then on DRUM the kit's and each pad's in
/// `PadBlock::ALL` order. The mixer's: each Part's mix, each Part's sends,
/// then the buses.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.block_at"))]
fn block_at(s: Schema, mut i: usize) -> Option<EdBlock> {
    let engine = match s {
        Schema::Engine(e) => e,
        // The matrix keeps no values: ROUTES keeps its cells.
        Schema::Matrix(_) => return None,
        Schema::Mixer => {
            let parts = PartId::ALL.len();
            if let Some(&p) = PartId::ALL.get(i) {
                return Some(EdBlock::Part(p, PartBlock::Mix));
            }
            i -= parts;
            if let Some(&p) = PartId::ALL.get(i) {
                return Some(EdBlock::Part(p, PartBlock::Sends));
            }
            i -= parts;
            return BusBlock::ALL.get(i).map(|&b| EdBlock::Bus(b));
        }
    };
    let mut b = 0;
    while let Some(&r) = BlockRef::ALL.get(b) {
        b += 1;
        if let Some(s) = SoundBlock::new(r)
            && EdBlock::Sound(s).reachable(engine)
        {
            if i == 0 {
                return Some(EdBlock::Sound(s));
            }
            i -= 1;
        }
    }
    if engine != EngineType::Drum {
        return None;
    }
    if i == 0 {
        return SoundBlock::new(BlockRef::Kit).map(EdBlock::Sound);
    }
    i -= 1;
    let pad = *PadId::ALL.get(i / PadBlock::ALL.len())?;
    Some(EdBlock::Pad(
        pad,
        *PadBlock::ALL.get(i % PadBlock::ALL.len())?,
    ))
}

/// Slot `a` takes in `s`'s copy.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.slot_of"))]
fn slot_of(s: Schema, a: EdAddr) -> Option<usize> {
    let mut n = 0;
    let mut i = 0;
    while let Some(at) = block_at(s, i) {
        i += 1;
        if at == a.at() {
            return at.slot(a.id()).map(|k| n + k);
        }
        n += at.width();
    }
    None
}

/// The slots `describe` lists a param for: the editor is told only of what
/// it was told exists, and `set` takes only these. Built from `describe`'s
/// own walk, so the two can't disagree.
pub(super) struct Described {
    schema: Schema,
    mask: [u32; MASK_WORDS],
}

impl Described {
    /// Note that `describe` lists `a`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.mark"))]
    pub(super) fn mark(&mut self, a: EdAddr) {
        if let Some(n) = slot_of(self.schema, a)
            && let Some(w) = self.mask.get_mut(n / 32)
        {
            *w |= 1 << (n % 32);
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.has_slot"))]
    fn has_slot(&self, n: usize) -> bool {
        self.mask
            .get(n / 32)
            .is_some_and(|w| w & (1 << (n % 32)) != 0)
    }

    /// `describe` lists `a` for this schema.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.has"))]
    pub(super) fn has(&self, a: EdAddr) -> bool {
        slot_of(self.schema, a).is_some_and(|n| self.has_slot(n))
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.built"))]
    pub(super) fn built(s: Schema) -> Described {
        let mut d = Described {
            schema: s,
            mask: [0; MASK_WORDS],
        };
        describe::mark_described(s, &mut d);
        d
    }
}

/// `d` for `s`: kept, else built now (another engine's dropped).
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.since.described_in")
)]
fn described_in(d: &mut Option<Described>, s: Schema) -> &Described {
    if d.as_ref().is_some_and(|d| d.schema != s) {
        *d = None;
    }
    d.get_or_insert_with(|| Described::built(s))
}

/// What an answer's sections read: the box, its cells' context, and the
/// refusals PART says.
#[derive(Clone, Copy)]
struct Reading<'a> {
    ui: &'a UiState,
    own: &'a CellOwn,
    refused: &'a Refusals,
}

/// The most panels an engine reports: DRUM's FX, its ENVs and LFOs, and
/// each pad's filter and machine.
const PANELS: usize = 1 + EnvSlot::ALL.len() + LfoSlot::ALL.len() + 2 * PadId::ALL.len();

/// Panel `i` of `engine`'s, in the order `since` lists them: names from
/// `describe`'s own table (`PanelName`).
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.shown_at"))]
fn shown_at(engine: EngineType, mut i: usize) -> Option<Shown> {
    let filter = match engine {
        EngineType::Drum => 0,
        EngineType::Algo | EngineType::Modal | EngineType::Tape | EngineType::Va => 1,
    };
    if i < filter {
        return Some(Shown::Filter);
    }
    i -= filter;
    if i == 0 {
        return Some(Shown::Fx);
    }
    i -= 1;
    if let Some(&s) = EnvSlot::ALL.get(i) {
        return Some(Shown::Env(s));
    }
    i -= EnvSlot::ALL.len();
    if let Some(&s) = LfoSlot::ALL.get(i) {
        return Some(Shown::Lfo(s));
    }
    i -= LfoSlot::ALL.len();
    match engine {
        EngineType::Algo => Op::ALL.get(i).map(|&o| Shown::Op(o)),
        EngineType::Modal => (i == 0).then_some(Shown::Resonator),
        EngineType::Tape => (i == 0).then_some(Shown::Machine),
        EngineType::Va => None,
        EngineType::Drum => {
            let pad = *PadId::ALL.get(i / 2)?;
            Some(if i.is_multiple_of(2) {
                Shown::PadFilter(pad)
            } else {
                Shown::PadMachine(pad)
            })
        }
    }
}

/// A panel `since` reports a variant of: a variant panel, never a page's
/// own cells or a route (`PanelName` names those too, for `describe`).
#[derive(Clone, Copy)]
enum Shown {
    Filter,
    Fx,
    Env(EnvSlot),
    Lfo(LfoSlot),
    Op(Op),
    Resonator,
    Machine,
    PadFilter(PadId),
    PadMachine(PadId),
}

impl Shown {
    /// Its name, from `describe`'s one table.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.since.shown_name")
    )]
    fn name(self) -> PanelName {
        match self {
            Shown::Filter => PanelName::Filter,
            Shown::Fx => PanelName::Fx,
            Shown::Env(s) => PanelName::Env(s),
            Shown::Lfo(s) => PanelName::Lfo(s),
            Shown::Op(o) => PanelName::Op(o),
            Shown::Resonator => PanelName::Resonator,
            Shown::Machine => PanelName::Machine,
            Shown::PadFilter(p) => PanelName::PadFilter(p),
            Shown::PadMachine(p) => PanelName::PadMachine(p),
        }
    }
}

/// Panel `n`'s variant now.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.since.variant_of")
)]
fn variant_of(n: Shown, c: &SlotCtx, kit: &Kit) -> &'static str {
    match n {
        Shown::Filter => filter_variant(c.kind),
        Shown::Fx => fx_variant(c.fx),
        Shown::Env(s) => env_variant(c.envs[s.index()]),
        Shown::Lfo(s) => lfo_variant(c.lfos[s.index()]),
        Shown::Op(o) => op_variant(c.op_kinds[o.index()]),
        Shown::Resonator => modal_variant(c.model),
        Shown::Machine => tape_variant(c.tape),
        Shown::PadFilter(p) => filter_variant(kit.pad(p).sound().filter().kind()),
        Shown::PadMachine(p) => machine_variant(kit.pad(p).machine()),
    }
}

lit!(
    /// MIXER's header, whole.
    MIXER = "mixer\n"
);
lit!(
    /// After `OK <m>`: sections are owed.
    MORE = " more"
);
lit!(
    /// The answer's last line's head.
    OK = "OK "
);

/// What a pass of `since`'s answer does with it.
#[derive(Clone, Copy)]
enum Pass {
    /// Count its bytes; keep nothing.
    Measure,
    /// Write it, and keep what it said.
    Commit,
}

/// A part of `since`'s answer, with its own copy and its own rule for
/// answering whole, in the order an answer takes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    /// The followed Part's Sound (and kit): protocol 3's answer.
    Part,
    /// The followed Part's mod matrix: every present cell.
    Routes,
    /// Every Part's mix and sends, the buses.
    Mixer,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Part, Section::Routes, Section::Mixer];

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.bit"))]
    const fn bit(self) -> u8 {
        match self {
            Section::Part => 1,
            Section::Routes => 2,
            Section::Mixer => 4,
        }
    }

    /// The section that reports `a`; `None` for one no section reports
    /// yet (SETTINGS' and ORBIT's come with their slices).
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.of"))]
    pub(super) const fn of(a: EdBlock) -> Option<Section> {
        match a {
            EdBlock::Sound(_) | EdBlock::Pad(..) => Some(Section::Part),
            EdBlock::Part(..) | EdBlock::Bus(_) => Some(Section::Mixer),
            EdBlock::Global(_) | EdBlock::Orbit(_) => None,
        }
    }
}

/// Sections, as a set: the ones an answer owes.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Sections(u8);

impl Sections {
    const NONE: Sections = Sections(0);
    const ALL: Sections = {
        let mut b = 0;
        let mut i = 0;
        while i < Section::ALL.len() {
            b |= Section::ALL[i].bit();
            i += 1;
        }
        Sections(b)
    };

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.has_sec"))]
    const fn has(self, s: Section) -> bool {
        self.0 & s.bit() != 0
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.with"))]
    const fn with(self, s: Section) -> Sections {
        Sections(self.0 | s.bit())
    }
}

/// What the Part looked like when last reported.
#[derive(Clone, Copy)]
struct Seen {
    part: PartId,
    engine: EngineType,
    panels: [&'static str; PANELS],
    /// What the box's text reads beside the bits (`cell_text`), as
    /// `value::text_ctx` has it: the cells resolve against it, so a change
    /// moves texts under unchanged bits and the values are answered whole.
    ctx: SlotCtx,
    /// Bank 2 can't play: `closed_text` reads FILTER.KIND and VOICEFX.TYPE as
    /// NEEDS UPDATE then, so it moves those texts under unchanged bits.
    closed: bool,
}

/// What the mixer's texts read beside the bits, when last reported: a
/// change answers MIXER whole.
#[derive(Clone, Copy, PartialEq, Eq)]
struct MixSeen {
    rev: ReverbType,
    chorus: ChorusMode,
    delay: DelayType,
    plate: bool,
    closed: bool,
}

/// What the reverb's panel texts read beyond their own values, the box's
/// `rev_text_key`: PRE-DLY's pull in HALL's plate zone and the clock a
/// synced cell's note reads. A change answers the reverb's values again.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.rev_key"))]
fn rev_key(ui: &UiState, own: &CellOwn) -> u64 {
    let pre = crate::dsp::reverb::plate_pull(&ui.project().perf().fx.reverb).predelay;
    // The tempo's bits and the fold: `meter::clock_word` would be bank 1's
    // out of line, and the box's key is finer than either changes.
    let (t, fold) = own.rev_clock;
    let clock = t.0.to_bits() ^ u32::from(fold.0 as u8) << 24;
    u64::from(pre.to_bits()) | u64::from(clock) << 32
}

/// PART's copy: protocol 3's.
struct PartCopy {
    seen: Option<Seen>,
    last: [Bits; SLOTS],
    /// Built on the first answer that has values, again for another engine.
    described: Option<Described>,
}

/// ROUTES' copy: the Part's routes as last reported, and whose. `kept`
/// is uninit while `seen` is `None`: `EdState::new`, which bank 1 builds
/// in place, writes no copy.
struct RoutesCopy {
    seen: Option<(PartId, EngineType)>,
    kept: MaybeUninit<Routes>,
}

impl RoutesCopy {
    /// The copy, if one was kept for `(part, engine)`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.routes_of"))]
    fn of(&self, part: PartId, engine: EngineType) -> Option<&Routes> {
        match self.seen {
            // SAFETY: `seen` is set only by `keep`, after it writes `kept`.
            Some(s) if s == (part, engine) => Some(unsafe { self.kept.assume_init_ref() }),
            Some(_) | None => None,
        }
    }

    /// `r` kept for `(part, engine)`.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.since.routes_keep")
    )]
    fn keep(&mut self, part: PartId, engine: EngineType, r: Routes) {
        self.kept.write(r);
        self.seen = Some((part, engine));
    }
}

/// MIXER's copy.
struct MixerCopy {
    seen: Option<MixSeen>,
    rev_key: u64,
    last: [Bits; MIXER_SLOTS],
    described: Option<Described>,
}

/// What `ed since` keeps: the last answer's number, the sections it still
/// owes whole, and each section's copy. The shell owns it, in `EdState`.
pub struct EdSince {
    issued: Option<Gen>,
    owed: Sections,
    part: PartCopy,
    routes: RoutesCopy,
    mixer: MixerCopy,
}

impl EdSince {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.new.1"))]
    pub const fn new() -> EdSince {
        EdSince {
            issued: None,
            owed: Sections::ALL,
            part: PartCopy {
                seen: None,
                last: [Bits(0); SLOTS],
                described: None,
            },
            routes: RoutesCopy {
                seen: None,
                kept: MaybeUninit::uninit(),
            },
            mixer: MixerCopy {
                seen: None,
                rev_key: 0,
                last: [Bits(0); MIXER_SLOTS],
                described: None,
            },
        }
    }

    /// What `describe` lists for `s`, built once a schema.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.described"))]
    pub(super) fn described(&mut self, s: Schema) -> &Described {
        match s {
            // The matrix describes no params: its cells are its sources'
            // and its destinations' (the engine's `d`-flagged params).
            Schema::Engine(_) | Schema::Matrix(_) => described_in(&mut self.part.described, s),
            Schema::Mixer => described_in(&mut self.mixer.described, s),
        }
    }

    /// `describe` lists `a` for the Part followed on `engine`: `set` takes
    /// only these.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.lists"))]
    pub(super) fn lists(&mut self, engine: EngineType, a: EdAddr) -> bool {
        let s = match Section::of(a.at()) {
            Some(Section::Part) => Schema::Engine(engine),
            Some(Section::Mixer) => Schema::Mixer,
            // No address is a route's: those are cells (`ed route`).
            Some(Section::Routes) | None => return false,
        };
        a.at().reachable(engine) && self.described(s).has(a)
    }

    /// The whole answer, `OK <m>` included.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.answer"))]
    pub(super) fn answer(
        &mut self,
        held: Option<Gen>,
        ui: &UiState,
        refused: &mut Refusals,
        w: &mut dyn Write,
    ) -> fmt::Result {
        self.answer_within(held, ui, refused, w, ANSWER_BYTES_MAX)
    }

    /// The answer, at most `max` bytes: each section measured first
    /// (writing nothing, keeping nothing), then taken in order while the
    /// answer fits, the rest owed (`OK <m> more`). A section past `max` by
    /// itself is `ERR ed since too large`, and nothing is kept.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.since.answer_within")
    )]
    pub(super) fn answer_within(
        &mut self,
        held: Option<Gen>,
        ui: &UiState,
        refused: &mut Refusals,
        w: &mut dyn Write,
        max: usize,
    ) -> fmt::Result {
        let known = held.is_some() && held == self.issued;
        let owed = if known { self.owed } else { Sections::ALL };
        let own = ui.cell_own();
        let m = match self.issued {
            Some(g) => g.next(),
            None => Gen::FIRST,
        };
        // The empty line and the longest `OK` line, always room for both.
        let tail = "\nOK 999999999 more\n".len();
        let room = max.saturating_sub(tail);
        let (mut take, mut total, mut whole) = (Sections::NONE, 0, false);
        for s in Section::ALL {
            let mut size = ByteCount(0);
            let at = Reading {
                ui,
                own: &own,
                refused,
            };
            let w_s = self.section(s, owed.has(s), at, Pass::Measure, &mut size)?;
            if size.0 > room {
                return w.write_str("ERR ed since too large\n");
            }
            if total + size.0 <= room {
                take = take.with(s);
                total += size.0;
                whole |= w_s;
            }
        }
        self.issued = Some(m);
        // What isn't taken is owed: whole next time.
        self.owed = Sections(Sections::ALL.0 & !take.0);
        if whole {
            // The editor ignores an empty line. It ends a fragment a cut-off
            // answer left, so a header is never glued onto it.
            writeln!(w)?;
        }
        for s in Section::ALL {
            if take.has(s) {
                let at = Reading {
                    ui,
                    own: &own,
                    refused,
                };
                self.section(s, owed.has(s), at, Pass::Commit, w)?;
            }
        }
        // Said, stale or not the followed Part's: none kept past an answer.
        refused.clear();
        w.write_str(OK.get())?;
        super::lit::uint(w, m.get())?;
        if self.owed != Sections::NONE {
            w.write_str(MORE.get())?;
        }
        w.write_char('\n')
    }

    /// Section `s`'s lines, whole when `owed` or its own rule says so;
    /// whether it was whole.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.section"))]
    fn section(
        &mut self,
        s: Section,
        owed: bool,
        at: Reading<'_>,
        pass: Pass,
        w: &mut dyn Write,
    ) -> Result<bool, fmt::Error> {
        let Reading { ui, own, refused } = at;
        match s {
            Section::Part => self.part_section(owed, ui, own, refused, pass, w),
            Section::Routes => self.routes_section(owed, ui, pass, w),
            Section::Mixer => self.mixer_section(owed, ui, own, pass, w),
        }
    }

    /// PART: the followed Part's panels, values and TAPE's source.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.since.part_section")
    )]
    fn part_section(
        &mut self,
        owed: bool,
        ui: &UiState,
        own: &CellOwn,
        refused: &Refusals,
        pass: Pass,
        w: &mut dyn Write,
    ) -> Result<bool, fmt::Error> {
        let known = if owed { None } else { self.part.seen };
        let part = ui.active_part;
        let engine = engine_of(ui);
        // Another Part or engine is a new Sound: all of it.
        let same = known.is_some_and(|k| k.part == part && k.engine == engine);
        let ctx = text_ctx(ui, own);
        let closed = ui.bank2().is_some_and(|b| !b.is_open());
        let now = panels(engine, &ctx, ui.project().kit(part));
        let was = match known {
            Some(k) if same => k.panels,
            _ => [""; PANELS],
        };
        // The texts too: whole again if the cells' context moved, or a
        // panel did (a pad's machine labels its knobs).
        let texts = known
            .is_some_and(|k| same && k.ctx == ctx && k.closed == closed && same_panels(&now, &was));
        if let Pass::Commit = pass {
            self.part.seen = Some(Seen {
                part,
                engine,
                panels: now,
                ctx,
                closed,
            });
        }
        if !same {
            writeln!(w, "part {} {}", part.number(), engine.disk_ident())?;
        }
        let mut i = 0;
        while let Some(s) = shown_at(engine, i) {
            if let (Some(n), Some(v)) = (now.get(i), was.get(i))
                && n != v
            {
                w.write_str("panel ")?;
                s.name().write(w)?;
                w.write_char(' ')?;
                describe::variant_token(n, w)?;
                w.write_char('\n')?;
            }
            i += 1;
        }
        let d = described_in(&mut self.part.described, Schema::Engine(engine));
        values(
            ui,
            own,
            Copy {
                schema: Schema::Engine(engine),
                last: &mut self.part.last,
                described: d,
            },
            texts,
            pass,
            w,
        )?;
        if !texts && let Some(t) = source_text(ui, own) {
            writeln!(w, "source \"{}\"", t.as_str())?;
        }
        // A whole PART is a new start: the refusals before it are stale.
        if same {
            refused.write(part, w)?;
        }
        Ok(!same)
    }

    /// ROUTES: the followed Part's present cells, whole after another Part
    /// or engine; else each cell whose amount moved, and each gone,
    /// compared by (source, destination).
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.since.routes_section")
    )]
    fn routes_section(
        &mut self,
        owed: bool,
        ui: &UiState,
        pass: Pass,
        w: &mut dyn Write,
    ) -> Result<bool, fmt::Error> {
        let part = ui.active_part;
        let engine = engine_of(ui);
        let sound = &ui.project().part(part).sound;
        let live = Routes {
            mods: sound.mod_state.clone(),
            taps: sound.params.taps,
        };
        let kept = match owed {
            false => self.routes.of(part, engine),
            true => None,
        };
        match kept {
            None => {
                route::header(part, w)?;
                live.each(engine, |c, a| route::line(c, a, w))?;
            }
            Some(k) => {
                live.each(engine, |c, a| match k.get(c) == Some(a) {
                    true => Ok(()),
                    false => route::line(c, a, w),
                })?;
                k.each(engine, |c, _| match live.get(c) {
                    Some(_) => Ok(()),
                    None => route::unline(c, w),
                })?;
            }
        }
        let whole = kept.is_none();
        if let Pass::Commit = pass {
            self.routes.keep(part, engine, live);
        }
        Ok(whole)
    }

    /// MIXER: every Part's mix and sends, the buses, the reverb's panel.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.since.mixer_section")
    )]
    fn mixer_section(
        &mut self,
        owed: bool,
        ui: &UiState,
        own: &CellOwn,
        pass: Pass,
        w: &mut dyn Write,
    ) -> Result<bool, fmt::Error> {
        let now = MixSeen {
            rev: own.ctx.rev,
            chorus: own.ctx.chorus,
            delay: own.ctx.delay,
            plate: own.ctx.plate,
            closed: ui.bank2().is_some_and(|b| !b.is_open()),
        };
        let whole = owed || self.mixer.seen != Some(now);
        let key = rev_key(ui, own);
        let rev_moved = !whole && self.mixer.rev_key != key;
        if let Pass::Commit = pass {
            self.mixer.seen = Some(now);
            self.mixer.rev_key = key;
        }
        if whole {
            w.write_str(MIXER.get())?;
            w.write_str("panel ")?;
            PanelName::Reverb.write(w)?;
            w.write_char(' ')?;
            describe::variant_token(reverb_variant(now.rev), w)?;
            w.write_char('\n')?;
        }
        let d = described_in(&mut self.mixer.described, Schema::Mixer);
        let copy = Copy {
            schema: Schema::Mixer,
            last: &mut self.mixer.last,
            described: d,
        };
        match rev_moved {
            false => values(ui, own, copy, !whole, pass, w)?,
            true => values_but(ui, own, copy, EdBlock::Bus(BusBlock::Reverb), pass, w)?,
        }
        Ok(whole)
    }
}

impl Default for EdSince {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.default"))]
    fn default() -> Self {
        Self::new()
    }
}

/// A section's copy of values, with what `describe` lists in it.
struct Copy<'a> {
    schema: Schema,
    last: &'a mut [Bits],
    described: &'a Described,
}

/// The `val` lines of what differs from `c`, all of them unless `same`
/// (the values and their texts' context as last reported); the copy is
/// brought up to date on `Commit`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.values"))]
fn values(
    ui: &UiState,
    own: &CellOwn,
    c: Copy<'_>,
    same: bool,
    pass: Pass,
    w: &mut dyn Write,
) -> fmt::Result {
    walk(ui, own, c, |_, moved| moved || !same, pass, w)
}

/// As `values`, with every value of block `again` answered whatever its
/// bits: its texts moved under them.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.since.values_but")
)]
fn values_but(
    ui: &UiState,
    own: &CellOwn,
    c: Copy<'_>,
    again: EdBlock,
    pass: Pass,
    w: &mut dyn Write,
) -> fmt::Result {
    walk(ui, own, c, |at, moved| moved || at == again, pass, w)
}

/// Each listed value of `c`'s schema, answered where `say(block, moved)`.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.walk"))]
fn walk(
    ui: &UiState,
    own: &CellOwn,
    c: Copy<'_>,
    say: impl Fn(EdBlock, bool) -> bool,
    pass: Pass,
    w: &mut dyn Write,
) -> fmt::Result {
    let (mut slot, mut b) = (0usize, 0);
    while let Some(at) = block_at(c.schema, b) {
        b += 1;
        let width = at.width();
        let Some(blk) = block(ui, at) else {
            let mut i = 0;
            while matches!(pass, Pass::Commit) && i < width {
                if let Some(last) = c.last.get_mut(slot + i) {
                    *last = Bits::ABSENT;
                }
                i += 1;
            }
            slot += width;
            continue;
        };
        let mut i = 0;
        while let Some(id) = at.id_at(i) {
            let now = Bits(blk.get(id).to_bits());
            if c.described.has_slot(slot + i)
                && let Some(last) = c.last.get_mut(slot + i)
                && say(at, *last != now)
            {
                if let Pass::Commit = pass {
                    *last = now;
                }
                if let Some(a) = EdAddr::new(at, id)
                    && let Some((norm, stored, text)) = reading(a, ui, own)
                {
                    line(a, norm, stored, text.as_str(), w)?;
                }
            }
            i += 1;
        }
        slot += width;
    }
    Ok(())
}

/// Every panel as it was: one `str` compare each (a whole-array compare
/// would be a slice compare instantiated into bank 1).
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.since.same_panels")
)]
fn same_panels(now: &[&'static str; PANELS], was: &[&'static str; PANELS]) -> bool {
    let mut i = 0;
    while i < PANELS {
        if now[i] != was[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Each panel's variant under `engine`, in `shown_at` order; the rest empty.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.panels"))]
fn panels(engine: EngineType, c: &SlotCtx, kit: &Kit) -> [&'static str; PANELS] {
    let mut v = [""; PANELS];
    let mut i = 0;
    while let Some(s) = shown_at(engine, i) {
        if let Some(slot) = v.get_mut(i) {
            *slot = variant_of(s, c, kit);
        }
        i += 1;
    }
    v
}

/// How many slots `s`'s blocks take: for the test that `SLOTS` and
/// `MIXER_SLOTS` hold them.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.since.slots_used")
)]
pub fn slots_used(s: Schema) -> usize {
    let mut n = 0;
    let mut i = 0;
    while let Some(at) = block_at(s, i) {
        i += 1;
        n += at.width();
    }
    n
}

/// How many panels `engine` reports: for the test that `PANELS` holds them.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.since.panels_used")
)]
pub fn panels_used(engine: EngineType) -> usize {
    let mut i = 0;
    while shown_at(engine, i).is_some() {
        i += 1;
    }
    i
}

/// `PANELS`, for the same test.
pub const PANELS_MAX: usize = PANELS;

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer past the bound is one `ERR` line, nothing kept: the next
    /// ask of the same number is still answered whole.
    #[test]
    fn too_large_is_a_refusal_and_keeps_nothing() {
        let ui = UiState::new();
        let mut s = EdSince::new();
        let mut r = Refusals::new();
        let mut out = alloc_free::Buf::new();
        s.answer_within(None, &ui, &mut r, &mut out, 100).unwrap();
        assert_eq!(out.as_str(), "ERR ed since too large\n");
        assert_eq!(s.issued, None);
        assert!(s.part.seen.is_none());
        let mut out = alloc_free::Buf::new();
        s.answer_within(None, &ui, &mut r, &mut out, ANSWER_BYTES_MAX)
            .unwrap();
        assert!(
            out.as_str().starts_with("\npart 1 ALGO\n"),
            "{}",
            out.as_str()
        );
        assert!(out.as_str().ends_with("OK 1\n"));
    }

    /// The answer's lines and its `OK` word.
    fn ask(s: &mut EdSince, ui: &UiState, held: Option<Gen>, max: usize) -> alloc_free::Buf {
        let mut out = alloc_free::Buf::new();
        s.answer_within(held, ui, &mut Refusals::new(), &mut out, max)
            .unwrap();
        out
    }

    fn last_line(b: &alloc_free::Buf) -> &str {
        b.as_str().trim_end().rsplit('\n').next().unwrap()
    }

    /// Past the bound, sections page: PART first, ROUTES and MIXER owed,
    /// then those whole with PART's (empty) diff. Nothing is lost.
    #[test]
    fn a_whole_answer_past_the_bound_pages() {
        let ui = UiState::new();
        let mut s = EdSince::new();
        let mut whole = ByteCount(0);
        let mut all = EdSince::new();
        all.answer_within(
            None,
            &ui,
            &mut Refusals::new(),
            &mut whole,
            ANSWER_BYTES_MAX,
        )
        .unwrap();
        let mut part = ByteCount(0);
        EdSince::new()
            .section(
                Section::Part,
                true,
                Reading {
                    ui: &ui,
                    own: &ui.cell_own(),
                    refused: &Refusals::new(),
                },
                Pass::Measure,
                &mut part,
            )
            .unwrap();
        // Room for PART, not for both.
        let max = part.0 + 64;
        assert!(whole.0 > max);
        let a = ask(&mut s, &ui, None, max);
        assert!(a.as_str().starts_with("\npart 1 ALGO\n"), "{}", a.as_str());
        assert!(!a.as_str().contains("\nmixer\n"));
        assert_eq!(last_line(&a), "OK 1 more");
        let b = ask(&mut s, &ui, Gen::new(1), max);
        assert!(b.as_str().starts_with("\nroutes 1\n"), "{}", b.as_str());
        assert!(b.as_str().contains("\nmixer\n"), "{}", b.as_str());
        assert!(!b.as_str().contains("part 1"));
        assert_eq!(last_line(&b), "OK 2");
        // Now both diff: nothing.
        let c = ask(&mut s, &ui, Gen::new(2), max);
        assert_eq!(c.as_str(), "OK 3\n");
    }

    /// A stale number mid-paging starts over: every section owed.
    #[test]
    fn a_stale_number_mid_paging_starts_over() {
        let ui = UiState::new();
        let mut s = EdSince::new();
        let mut part = ByteCount(0);
        EdSince::new()
            .section(
                Section::Part,
                true,
                Reading {
                    ui: &ui,
                    own: &ui.cell_own(),
                    refused: &Refusals::new(),
                },
                Pass::Measure,
                &mut part,
            )
            .unwrap();
        let max = part.0 + 64;
        let a = ask(&mut s, &ui, None, max);
        assert_eq!(last_line(&a), "OK 1 more");
        // The page with MIXER never arrived: the editor asks with 1 again,
        // after a newer answer it never saw.
        let _lost = ask(&mut s, &ui, Gen::new(1), max);
        let again = ask(&mut s, &ui, Gen::new(1), max);
        assert!(
            again.as_str().starts_with("\npart 1 ALGO\n"),
            "{}",
            again.as_str()
        );
        assert_eq!(last_line(&again), "OK 3 more");
        let rest = ask(&mut s, &ui, Gen::new(3), max);
        assert!(rest.as_str().starts_with("\nroutes 1\n"));
        assert!(rest.as_str().contains("\nmixer\n"));
        assert_eq!(last_line(&rest), "OK 4");
    }

    mod alloc_free {
        /// A big fixed buffer: the lib's tests have no `String` here.
        pub struct Buf {
            b: [u8; 32768],
            n: usize,
        }

        impl Buf {
            pub fn new() -> Buf {
                Buf {
                    b: [0; 32768],
                    n: 0,
                }
            }

            pub fn as_str(&self) -> &str {
                core::str::from_utf8(&self.b[..self.n]).unwrap_or("")
            }
        }

        impl core::fmt::Write for Buf {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                let end = self.n + s.len();
                self.b
                    .get_mut(self.n..end)
                    .ok_or(core::fmt::Error)?
                    .copy_from_slice(s.as_bytes());
                self.n = end;
                Ok(())
            }
        }
    }
}
