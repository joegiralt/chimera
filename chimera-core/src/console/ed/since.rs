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
//! OK <m>
//! ```
//!
//! `m` is the new number. A `val` is `ed get`'s own line (`value::reading`),
//! so the two never disagree. A non-ALGO Part answers only its `part` line.
//! The number moves before the answer is written, so an answer that never
//! reached the editor leaves it asking with a stale one: everything again.
//!
//! Runs only in the console's answer path, never in audio. The unit never
//! pushes: no sequence numbers, timers or hashes.

use core::fmt::{self, Write};

use super::value::{line, reading};
use super::{EdAddr, describe};
use crate::addr::{BlockRead, BlockRef, ParamAddr};
use crate::project::PartId;
use crate::ui::UiState;
use crate::ui::view::SlotCtx;
use chimera_engine::block::{DiskCode, ParamId};
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
    pub const fn new(n: u32) -> Option<Gen> {
        if n == 0 || n > Gen::LAST {
            None
        } else {
            Some(Gen(n))
        }
    }

    pub const fn next(self) -> Gen {
        if self.0 == Gen::LAST {
            Gen::FIRST
        } else {
            Gen(self.0 + 1)
        }
    }

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

/// The values kept: one for each param of every block an ALGO Sound shows
/// (`in_schema`), in `BlockRef::ALL` order, a block it lacks included (so a
/// slot never moves). A test says it holds them.
pub const SLOTS: usize = 256;

/// The panels a Sound shows a variant of: FILTER, FX, three ENVs, three
/// LFOs, six OPs, in `describe`'s names.
const PANELS: usize = 14;
const PANEL_NAMES: [&str; PANELS] = [
    "FILTER", "FX", "ENV1", "ENV2", "ENV3", "LFO1", "LFO2", "LFO3", "OP1", "OP2", "OP3", "OP4",
    "OP5", "OP6",
];

/// The blocks the editor's ALGO schema has: the other engines' params sit in
/// the Sound too, and are no part of it.
fn in_schema(b: BlockRef) -> bool {
    matches!(
        b,
        BlockRef::Algo
            | BlockRef::Filter
            | BlockRef::VoiceFx
            | BlockRef::AlgoOp(_)
            | BlockRef::Env(_)
            | BlockRef::Lfo(_)
            | BlockRef::Out
            | BlockRef::Pitch
    )
}

/// The slots `describe` lists a param for: the editor is told only of what
/// it was told exists. Built from `describe`'s own walk, so the two can't
/// disagree.
pub(super) struct Described([u32; SLOTS / 32]);

impl Described {
    /// Slot `block`'s param `id` takes in `last`: after the blocks before it
    /// that `in_schema` keeps.
    fn slot(block: BlockRef, id: ParamId) -> Option<usize> {
        let (mut n, mut b) = (0usize, 0);
        while let Some(&x) = BlockRef::ALL.get(b) {
            b += 1;
            if !in_schema(x) {
                continue;
            }
            if x == block {
                return block.specs().iter().position(|s| s.id == id).map(|i| n + i);
            }
            n += x.specs().len();
        }
        None
    }

    /// Note that `describe` lists `block`'s `id`.
    pub(super) fn mark(&mut self, block: BlockRef, id: ParamId) {
        if let Some(n) = Described::slot(block, id)
            && let Some(w) = self.0.get_mut(n / 32)
        {
            *w |= 1 << (n % 32);
        }
    }

    fn has(&self, n: usize) -> bool {
        self.0.get(n / 32).is_some_and(|w| w & (1 << (n % 32)) != 0)
    }

    fn built() -> Described {
        let mut d = Described([0; SLOTS / 32]);
        describe::mark_described(&mut d);
        d
    }
}

/// What the Part looked like when last reported.
#[derive(Clone, Copy)]
struct Seen {
    part: PartId,
    engine: EngineType,
    panels: [&'static str; PANELS],
    /// What the box's text reads beside the bits (`cell_text`): the cells
    /// resolve against it, so a change moves texts under unchanged bits and
    /// the values are answered whole. The reverb clock is read only by cells
    /// no ALGO page has (a test says so).
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
    /// Built on the first answer that has values.
    described: Option<Described>,
}

impl EdSince {
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
    fn known(&self, held: Option<Gen>) -> Option<Seen> {
        match (held, self.issued) {
            (Some(h), Some(i)) if h == i => self.seen,
            _ => None,
        }
    }

    /// The whole answer, `OK <m>` included.
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
        let engine = ui.project().part(part).sound.engine();
        let algo = engine == EngineType::Algo;
        // Another Part or engine is a new Sound: all of it.
        let same = match known {
            Some(k) => k.part == part && k.engine == engine,
            None => false,
        };
        let ctx = ui.cell_own().ctx;
        // The texts too: whole again if the cells' context moved.
        let closed = ui.bank2().is_some_and(|b| !b.is_open());
        let texts = match known {
            Some(k) => same && k.ctx == ctx && k.closed == closed,
            None => false,
        };
        let now = if algo { panels(ui) } else { [""; PANELS] };
        let was = match known {
            Some(k) if same => k.panels,
            _ => [""; PANELS],
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
            writeln!(w, "part {} {}", part.index() + 1, engine.disk_ident())?;
        }
        if algo {
            let mut i = 0;
            while i < PANELS {
                if now[i] != was[i] {
                    writeln!(w, "panel {} {}", PANEL_NAMES[i], now[i])?;
                }
                i += 1;
            }
            self.values(ui, texts, w)?;
        }
        writeln!(w, "OK {}", m.get())
    }

    /// The `val` lines of what differs from the copy, all of them unless
    /// `same` (the Sound and the texts' context as last reported); the copy
    /// is brought up to date.
    fn values(&mut self, ui: &UiState, same: bool, w: &mut dyn Write) -> fmt::Result {
        let own = ui.cell_own();
        let described = self.described.get_or_insert_with(Described::built);
        let (mut slot, mut b) = (0usize, 0);
        while let Some(&block) = BlockRef::ALL.get(b) {
            b += 1;
            if !in_schema(block) {
                continue;
            }
            let specs = block.specs();
            let Some(blk) = ui.params().block(block) else {
                let mut i = 0;
                while i < specs.len() {
                    if let Some(last) = self.last.get_mut(slot + i) {
                        *last = Bits::ABSENT;
                    }
                    i += 1;
                }
                slot += specs.len();
                continue;
            };
            let mut i = 0;
            while let Some(spec) = specs.get(i) {
                let now = Bits(blk.get(spec.id).to_bits());
                if described.has(slot + i)
                    && let Some(last) = self.last.get_mut(slot + i)
                    && (!same || *last != now)
                {
                    *last = now;
                    let a = EdAddr(ParamAddr::new(block, spec.id));
                    if let Some((norm, stored, text)) = reading(a, ui, &own) {
                        line(a, norm, stored, text.as_str(), w)?;
                    }
                }
                i += 1;
            }
            slot += specs.len();
        }
        Ok(())
    }
}

impl Default for EdSince {
    fn default() -> Self {
        Self::new()
    }
}

/// Each panel's variant under `ui`'s Sound, in `PANEL_NAMES` order.
fn panels(ui: &UiState) -> [&'static str; PANELS] {
    let c = ui.cell_own().ctx;
    let mut v = [""; PANELS];
    v[0] = describe::filter_variant(c.kind);
    v[1] = describe::fx_variant(c.fx);
    let mut i = 0;
    while i < 3 {
        v[2 + i] = describe::env_variant(c.envs[i]);
        v[5 + i] = describe::lfo_variant(c.lfos[i]);
        i += 1;
    }
    let mut i = 0;
    while i < 6 {
        v[8 + i] = describe::op_variant(c.op_kinds[i]);
        i += 1;
    }
    v
}

/// How many slots the blocks take: for the test that `SLOTS` holds them.
pub fn slots_used() -> usize {
    let mut n = 0;
    let mut b = 0;
    while let Some(&block) = BlockRef::ALL.get(b) {
        b += 1;
        if in_schema(block) {
            n += block.specs().len();
        }
    }
    n
}
