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
    PanelName, env_variant, filter_variant, fx_variant, lfo_variant, machine_variant,
    modal_variant, op_variant, tape_variant,
};
use super::value::{block, engine_of, line, reading, source_text, text_ctx};
use super::{EdAddr, describe};
use crate::addr::{BlockRef, Op};
use crate::console::ANSWER_BYTES_MAX;
use crate::dsp::drum::types::{Kit, PadId};
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::project::PartId;
use crate::ui::UiState;
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

/// What a pass of `since`'s answer does with it.
#[derive(Clone, Copy)]
enum Pass {
    /// Count its bytes; keep nothing.
    Measure,
    /// Write it, and keep what it said.
    Commit,
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
        self.answer_within(held, ui, w, ANSWER_BYTES_MAX)
    }

    /// The answer if it is at most `max` bytes, else `ERR ed since too
    /// large` and nothing kept: the answer is measured first (writing
    /// nothing, keeping nothing), so it is never cut by the answer clock
    /// mid-line. The editor shows the refusal and stops asking.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.since.answer_within")
    )]
    pub(super) fn answer_within(
        &mut self,
        held: Option<Gen>,
        ui: &UiState,
        w: &mut dyn Write,
        max: usize,
    ) -> fmt::Result {
        let mut size = ByteCount(0);
        self.pass(held, ui, Pass::Measure, &mut size)?;
        if size.0 > max {
            return w.write_str("ERR ed since too large\n");
        }
        self.pass(held, ui, Pass::Commit, w)
    }

    /// One pass of the answer: `Measure` writes to a counter and keeps
    /// nothing, `Commit` writes it and keeps what it said.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.since.pass"))]
    fn pass(
        &mut self,
        held: Option<Gen>,
        ui: &UiState,
        pass: Pass,
        w: &mut dyn Write,
    ) -> fmt::Result {
        let known = self.known(held);
        let m = match self.issued {
            Some(g) => g.next(),
            None => Gen::FIRST,
        };
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
        if let Pass::Commit = pass {
            self.issued = Some(m);
            self.seen = Some(Seen {
                part,
                engine,
                panels: now,
                ctx,
                closed,
            });
        }
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
                s.name().write(w)?;
                w.write_char(' ')?;
                describe::variant_token(n, w)?;
                w.write_char('\n')?;
            }
            i += 1;
        }
        self.values(ui, engine, texts, pass, w)?;
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
        pass: Pass,
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
                while matches!(pass, Pass::Commit) && i < width {
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
                    if let Pass::Commit = pass {
                        *last = now;
                    }
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
            *slot = variant_of(s, c, kit);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer past the bound is one `ERR` line, nothing kept: the next
    /// ask of the same number is still answered whole.
    #[test]
    fn too_large_is_a_refusal_and_keeps_nothing() {
        let ui = UiState::new();
        let mut s = EdSince::new();
        let mut out = alloc_free::Buf::new();
        s.answer_within(None, &ui, &mut out, 100).unwrap();
        assert_eq!(out.as_str(), "ERR ed since too large\n");
        assert_eq!(s.issued, None);
        assert!(s.seen.is_none());
        let mut out = alloc_free::Buf::new();
        s.answer_within(None, &ui, &mut out, ANSWER_BYTES_MAX)
            .unwrap();
        assert!(
            out.as_str().starts_with("\npart 1 ALGO\n"),
            "{}",
            out.as_str()
        );
        assert!(out.as_str().ends_with("OK 1\n"));
    }

    mod alloc_free {
        /// A big fixed buffer: the lib's tests have no `String` here.
        pub struct Buf {
            b: [u8; 16384],
            n: usize,
        }

        impl Buf {
            pub fn new() -> Buf {
                Buf {
                    b: [0; 16384],
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
