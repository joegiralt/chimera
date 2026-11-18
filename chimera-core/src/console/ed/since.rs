//! `ed since <n>`: the editor pulls what changed on the followed Part.
//! `<n>` is the number of the last answer it got (`0`: nothing yet). The
//! unit keeps one copy of what it last reported and that answer's number
//! (`Gen`); if `<n>` is it, the live Sound is compared with the copy, at
//! f32 bits, and only what differs is answered, else everything. The answer:
//!
//! ```text
//! (an empty line, opening every answer that isn't a diff)
//! part <n> <ENGINE>
//! panel <name> <variant>
//! val <addr> <normalized> <stored> "<text>"
//! source "<text>"
//! OK <m>
//! ```
//!
//! `m` is the new number. A `val` is `ed get`'s own line (`value::reading`),
//! so the two never disagree. `source` is TAPE's, in every whole answer.
//! The number moves before the answer is written, so an answer that never
//! reached the editor leaves it asking with a stale one: everything again.
//!
//! Runs only in the console's answer path, never in audio. The unit never
//! pushes: no sequence numbers, timers or hashes.

use core::fmt::{self, Write};

use super::addr::{EdBlock, PadBlock, SoundBlock};
use super::describe::{
    env_variant, filter_variant, fx_variant, lfo_variant, machine_variant, modal_variant,
    op_variant, tape_variant,
};
use super::value::{block, engine_of, line, reading, source_text, text_ctx};
use super::{EdAddr, describe};
use crate::addr::{BlockRef, Op};
use crate::dsp::drum::types::{Kit, PadId};
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::project::PartId;
use crate::ui::UiState;
use crate::ui::view::SlotCtx;
use chimera_engine::block::DiskCode;
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

/// The values kept: one for each param of every block the followed
/// engine's schema has (`in_engine`), in `block_at` order, a block the
/// Sound lacks included (so a slot never moves). DRUM's twelve pads take
/// the most; a test says it holds every engine's.
pub const SLOTS: usize = 711;
/// `Described`'s words.
const MASK_WORDS: usize = SLOTS.div_ceil(32);

/// The Sound's blocks an engine's editor shows: the other engines' params
/// sit in the Sound too, and are no part of it. The kit's and the pads'
/// are DRUM's.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.in_schema"))]
fn in_schema(engine: EngineType, b: BlockRef) -> bool {
    match b {
        BlockRef::Algo | BlockRef::AlgoOp(_) => engine == EngineType::Algo,
        BlockRef::Modal => engine == EngineType::Modal,
        BlockRef::TapeEngine => engine == EngineType::Tape,
        BlockRef::Va => engine == EngineType::Va,
        BlockRef::Kit
        | BlockRef::Pad
        | BlockRef::PadFilter
        | BlockRef::PadAmp
        | BlockRef::PadEnv
        | BlockRef::PadLfo
        | BlockRef::PadRoute(_) => engine == EngineType::Drum,
        BlockRef::Filter
        | BlockRef::VoiceFx
        | BlockRef::Env(_)
        | BlockRef::Lfo(_)
        | BlockRef::Out
        | BlockRef::Pitch => true,
        BlockRef::Chorus
        | BlockRef::Delay
        | BlockRef::Reverb
        | BlockRef::Tape
        | BlockRef::Comp
        | BlockRef::Part
        | BlockRef::PartMix(_)
        | BlockRef::Theme
        | BlockRef::OrbitChords
        | BlockRef::OrbitRing(_)
        | BlockRef::Tempo
        | BlockRef::Midi
        | BlockRef::Random => false,
    }
}

/// `at` is one of `engine`'s blocks.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.in_engine"))]
pub(super) fn in_engine(engine: EngineType, at: EdBlock) -> bool {
    in_schema(engine, at.block())
}

/// Block `i` of `engine`'s slots: the Sound's in `BlockRef::ALL` order,
/// then on DRUM the kit's and each pad's in `PadBlock::ALL` order.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.block_at"))]
fn block_at(engine: EngineType, mut i: usize) -> Option<EdBlock> {
    let mut b = 0;
    while let Some(&r) = BlockRef::ALL.get(b) {
        b += 1;
        if in_schema(engine, r)
            && let Some(s) = SoundBlock::new(r)
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

/// Slot `a` takes in `engine`'s copy.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.slot_of"))]
fn slot_of(engine: EngineType, a: EdAddr) -> Option<usize> {
    let mut n = 0;
    let mut i = 0;
    while let Some(at) = block_at(engine, i) {
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
    engine: EngineType,
    mask: [u32; MASK_WORDS],
}

impl Described {
    /// Note that `describe` lists `a`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.mark"))]
    pub(super) fn mark(&mut self, a: EdAddr) {
        if let Some(n) = slot_of(self.engine, a)
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

    /// `describe` lists `a` for this engine.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.has"))]
    pub(super) fn has(&self, a: EdAddr) -> bool {
        slot_of(self.engine, a).is_some_and(|n| self.has_slot(n))
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.built"))]
    pub(super) fn built(engine: EngineType) -> Described {
        let mut d = Described {
            engine,
            mask: [0; MASK_WORDS],
        };
        describe::mark_described(engine, &mut d);
        d
    }
}

/// A panel `since` reports the variant of: the Sound's, or a pad's.
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

/// The most panels an engine shows: DRUM's FX, three ENVs, three LFOs and
/// each pad's filter and machine.
const PANELS: usize = 1 + 3 + 3 + 2 * 12;
const _: () = assert!(PadId::ALL.len() == 12);

/// Panel `i` of `engine`'s, in the order `since` lists them.
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
            Some(match i % 2 {
                0 => Shown::PadFilter(pad),
                _ => Shown::PadMachine(pad),
            })
        }
    }
}

impl Shown {
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.since.write_name")
    )]
    fn write_name(self, w: &mut dyn Write) -> fmt::Result {
        match self {
            Shown::Filter => w.write_str("FILTER"),
            Shown::Fx => w.write_str("FX"),
            Shown::Env(s) => w.write_str(["ENV1", "ENV2", "ENV3"][s.index()]),
            Shown::Lfo(s) => w.write_str(["LFO1", "LFO2", "LFO3"][s.index()]),
            Shown::Op(o) => w.write_str(["OP1", "OP2", "OP3", "OP4", "OP5", "OP6"][o.index()]),
            Shown::Resonator => w.write_str("RESONATOR"),
            Shown::Machine => w.write_str("MACHINE"),
            Shown::PadFilter(p) => write!(w, "PAD{}_FLT", p.number()),
            Shown::PadMachine(p) => write!(w, "PAD{}_MCH", p.number()),
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.variant"))]
    fn variant(self, c: &SlotCtx, kit: &Kit) -> &'static str {
        match self {
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

/// What `ed since` keeps: the last answer's number, and what it said.
/// The shell owns it, in `EdState`.
pub struct EdSince {
    issued: Option<Gen>,
    seen: Option<Seen>,
    last: [Bits; SLOTS],
    /// Built on the first answer that has values, again for another engine.
    described: Option<Described>,
}

impl EdSince {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.new.1"))]
    pub const fn new() -> EdSince {
        EdSince {
            issued: None,
            seen: None,
            last: [Bits(0); SLOTS],
            described: None,
        }
    }

    /// The copy `held` is of: `None` if the editor holds nothing, or not
    /// the last answer.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.known"))]
    fn known(&self, held: Option<Gen>) -> Option<Seen> {
        match (held, self.issued) {
            (Some(h), Some(i)) if h == i => self.seen,
            _ => None,
        }
    }

    /// What `describe` lists for `engine`, built once an engine.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.described"))]
    pub(super) fn described(&mut self, engine: EngineType) -> &Described {
        let stale = self.described.as_ref().is_none_or(|d| d.engine != engine);
        if stale {
            self.described = None;
        }
        self.described
            .get_or_insert_with(|| Described::built(engine))
    }

    /// The whole answer, `OK <m>` included.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.answer"))]
    pub(super) fn answer(
        &mut self,
        held: Option<Gen>,
        ui: &UiState,
        w: &mut dyn Write,
    ) -> fmt::Result {
        let known = self.known(held);
        let m = match self.issued {
            Some(g) => g.next(),
            None => Gen::FIRST,
        };
        self.issued = Some(m);
        let part = ui.active_part;
        let engine = engine_of(ui);
        // Another Part or engine is a new Sound: all of it.
        let same = match known {
            Some(k) => k.part == part && k.engine == engine,
            None => false,
        };
        let own = ui.cell_own();
        let ctx = text_ctx(ui, &own);
        let closed = ui.bank2().is_some_and(|b| !b.is_open());
        let now = panels(engine, &ctx, ui.project().kit(part));
        let was = match known {
            Some(k) if same => k.panels,
            _ => [""; PANELS],
        };
        // The texts too: whole again if the cells' context moved, or a
        // panel did (a pad's machine labels its knobs).
        let texts = match known {
            Some(k) => same && k.ctx == ctx && k.closed == closed && same_panels(&now, &was),
            None => false,
        };
        self.seen = Some(Seen {
            part,
            engine,
            panels: now,
            ctx,
            closed,
        });
        if !same {
            // The editor ignores an empty line. It ends a fragment a cut-off
            // answer left, so `part` is never glued onto it.
            writeln!(w)?;
            writeln!(w, "part {} {}", part.number(), engine.disk_ident())?;
        }
        let mut i = 0;
        while let Some(s) = shown_at(engine, i) {
            if let (Some(n), Some(v)) = (now.get(i), was.get(i))
                && n != v
            {
                w.write_str("panel ")?;
                s.write_name(w)?;
                w.write_char(' ')?;
                describe::variant_token(n, w)?;
                w.write_char('\n')?;
            }
            i += 1;
        }
        self.values(ui, engine, texts, w)?;
        if !texts && let Some(t) = source_text(ui, &own) {
            writeln!(w, "source \"{}\"", t.as_str())?;
        }
        writeln!(w, "OK {}", m.get())
    }

    /// The `val` lines of what differs from the copy, all of them unless
    /// `same` (the Sound and the texts' context as last reported); the copy
    /// is brought up to date.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.values"))]
    fn values(
        &mut self,
        ui: &UiState,
        engine: EngineType,
        same: bool,
        w: &mut dyn Write,
    ) -> fmt::Result {
        let own = ui.cell_own();
        self.described(engine);
        let (mut slot, mut b) = (0usize, 0);
        while let Some(at) = block_at(engine, b) {
            b += 1;
            let width = at.width();
            let Some(blk) = block(ui, at) else {
                let mut i = 0;
                while i < width {
                    if let Some(last) = self.last.get_mut(slot + i) {
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
                let listed = self
                    .described
                    .as_ref()
                    .is_some_and(|d| d.has_slot(slot + i));
                if listed
                    && let Some(last) = self.last.get_mut(slot + i)
                    && (!same || *last != now)
                {
                    *last = now;
                    if let Some(a) = EdAddr::new(at, id)
                        && let Some((norm, stored, text)) = reading(a, ui, &own)
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
}

impl Default for EdSince {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.default"))]
    fn default() -> Self {
        Self::new()
    }
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
            *slot = s.variant(c, kit);
        }
        i += 1;
    }
    v
}

/// How many slots `engine`'s blocks take: for the test that `SLOTS`
/// holds them.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.since.slots_used")
)]
pub fn slots_used(engine: EngineType) -> usize {
    let mut n = 0;
    let mut i = 0;
    while let Some(at) = block_at(engine, i) {
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
