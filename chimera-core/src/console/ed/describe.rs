//! `ed describe <ENGINE> <page>`: one page's static schema, from the
//! engine's chain and the box's own cell resolver (`view::view`), never the
//! Sound. A page lists its panels, each a variant of what a cell resolves
//! against (`FILTER LADDER`, `ENV1 B-LFO`, `OP3 FORMANT`, `PAD3_MCH TOM`);
//! `-` for cells no variant changes.
//!
//! ```text
//! page <n> <LABEL> of <count>
//! panel <name> <variant>
//! param <addr> "<label>" <cont|choice|toggle> <lin|oct> <min> <max> <step> <default>
//! opt <addr> <index> "<name>"
//! route "<cell label>" next-slice
//! source "<cell label>"
//! ```
//!
//! ALGO's chain is `ALGO_FORMANT_CHAIN`, so the editor has every page any
//! operator mix can show. On DRUM a page that edits the pad is described
//! once a pad (`PAD3_KIT`), after the Part's pages (ADR 0285).

use core::fmt::{self, Write};

use super::EdAddr;
use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::{Block, ParamSpec, Scale, ValFmt};
use crate::dsp::algo::kinds::OpKind;
use crate::dsp::algo::waves::WaveId;
use crate::dsp::drum::sound::RouteId;
use crate::dsp::drum::types::{Machine, PadId};
use crate::dsp::filter::{FilterKind, KIND_NAMES};
use crate::dsp::modal::ResonatorMode;
use crate::dsp::modulator::{EnvForm, EnvSlot, EnvSpeed, Func, LfoForm, LfoSlot};
use crate::dsp::tape_engine::params::{TapeEngineParams, TapeMachine};
use crate::dsp::voice_fx::params::FxType;
use crate::params::{EngineType, FilterParams};
use crate::ui::block_def::{BlockDef, ChainDef2, SlotBinding};
use crate::ui::block_registry::{
    ALGO_FORMANT_CHAIN, DRUM_CHAIN, MODAL_PLUCK_CHAIN, TAPE_CHAIN, VA_CHAIN,
};
use crate::ui::view::{self, EnvKind, LfoKind, SlotCtx, View};
use chimera_engine::addr::{BlockRead, BlockView};
use chimera_engine::dsp::drum::params::PadParams;
use chimera_engine::dsp::modal::ModalParams;

/// The chain the editor walks for `engine`.
pub fn chain(engine: EngineType) -> &'static ChainDef2 {
    match engine {
        EngineType::Algo => &ALGO_FORMANT_CHAIN,
        EngineType::Modal => &MODAL_PLUCK_CHAIN,
        EngineType::Tape => &TAPE_CHAIN,
        EngineType::Va => &VA_CHAIN,
        EngineType::Drum => &DRUM_CHAIN,
    }
}

/// Whether a cell bound `b` resolves against the pad (DRUM's): a pad's
/// param, its machine's knobs, its filter's panel, its routes. FLT's
/// sub-pages' panel cells count: on DRUM they read the pad's KIND.
fn pad_binding(b: SlotBinding) -> bool {
    match b {
        SlotBinding::Param(a) => a.block.is_pad(),
        SlotBinding::SelectPad
        | SlotBinding::Knob(_)
        | SlotBinding::PadFilterPanel(_)
        | SlotBinding::FilterPanel(_)
        | SlotBinding::SelectRoute
        | SlotBinding::SelectedRoute(_) => true,
        SlotBinding::Empty
        | SlotBinding::SelectedOp(_)
        | SlotBinding::SelectOp
        | SlotBinding::Legacy(_)
        | SlotBinding::FxPanel(_)
        | SlotBinding::EnvPanel(..)
        | SlotBinding::LfoPanel(..)
        | SlotBinding::ModalPanel(..)
        | SlotBinding::OpPanel(..)
        | SlotBinding::Machine(_)
        | SlotBinding::Source
        | SlotBinding::ReverbPanel(..)
        | SlotBinding::SelectedRing(_)
        | SlotBinding::Orbit(_) => false,
    }
}

/// `def` is described once a pad on `engine`.
fn per_pad(engine: EngineType, def: &BlockDef) -> bool {
    let mut any = false;
    for s in &def.params {
        any |= pad_binding(s.binding);
    }
    engine == EngineType::Drum && any
}

/// Flat page `n` of the chain: each node's home, then its sub-pages.
fn def_in(c: &ChainDef2, mut n: usize) -> Option<&'static BlockDef> {
    for b in c.blocks {
        if n == 0 {
            return Some(b.def);
        }
        n -= 1;
        if let Some(d) = b.sub_pages.get(n) {
            return Some(d);
        }
        n -= b.sub_pages.len();
    }
    None
}

/// The chain's pages, `(the Part's, a pad's)`.
fn counts(engine: EngineType) -> (usize, usize) {
    let c = chain(engine);
    let (mut part, mut pad) = (0, 0);
    let mut n = 0;
    while let Some(def) = def_in(c, n) {
        n += 1;
        if per_pad(engine, def) {
            pad += 1;
        } else {
            part += 1;
        }
    }
    (part, pad)
}

/// The pages the editor walks for `engine`.
pub fn pages(engine: EngineType) -> u8 {
    let (part, pad) = counts(engine);
    let pads = if pad == 0 { 0 } else { PadId::ALL.len() };
    (part + pad * pads) as u8
}

/// Page `n`: the Part's pages in chain order, then each pad's.
pub(super) fn page_at(engine: EngineType, n: usize) -> Option<(&'static BlockDef, Option<PadId>)> {
    let c = chain(engine);
    let (part, pad) = counts(engine);
    let (want_pad, mut k) = if n < part {
        (None, n)
    } else if pad == 0 {
        return None;
    } else {
        let p = *PadId::ALL.get((n - part) / pad)?;
        (Some(p), (n - part) % pad)
    };
    let mut i = 0;
    while let Some(def) = def_in(c, i) {
        i += 1;
        if per_pad(engine, def) == want_pad.is_some() {
            if k == 0 {
                return Some((def, want_pad));
            }
            k -= 1;
        }
    }
    None
}

/// A variant as one token: a name's spaces as `-` (`TONE KICK` is
/// `TONE-KICK`). `since` writes them the same way.
pub(super) fn variant_token(name: &str, w: &mut dyn Write) -> fmt::Result {
    for c in name.chars() {
        w.write_char(if c == ' ' { '-' } else { c })?;
    }
    Ok(())
}

/// A panel's variant names; `since` reports the same ones.
pub fn filter_variant(k: FilterKind) -> &'static str {
    let i = FilterKind::BUILT.iter().position(|&b| b == k).unwrap_or(0);
    KIND_NAMES[i]
}

pub fn fx_variant(t: FxType) -> &'static str {
    let i = FxType::LIST.iter().position(|&b| b == t).unwrap_or(0);
    FxType::NAMES[i]
}

/// One name for each panel `mod_panel::env_panel` has.
pub fn env_variant(k: EnvKind) -> &'static str {
    match k {
        EnvKind::A(EnvSpeed::Fast) => "A-FAST",
        EnvKind::A(EnvSpeed::Med) => "A-MED",
        EnvKind::A(EnvSpeed::Slow) => "A-SLOW",
        EnvKind::B(Func::Env(_)) => "B-ENV",
        EnvKind::B(Func::Lfo(LfoForm::Lfv)) => "B-LFV",
        EnvKind::B(Func::Lfo(_)) => "B-LFO",
        EnvKind::B(Func::Burst(_)) => "B-BURST",
    }
}

pub fn lfo_variant(k: LfoKind) -> &'static str {
    match k {
        LfoKind::Classic => "CLASSIC",
        LfoKind::Func(LfoForm::Lfv) => "FUNC-LFV",
        LfoKind::Func(_) => "FUNC",
    }
}

pub fn op_variant(k: OpKind) -> &'static str {
    match k {
        OpKind::Table(_) => "TABLE",
        OpKind::Formant => "FORMANT",
        OpKind::Noise => "NOISE",
    }
}

/// Option `i`'s name of `a`'s choice: a variant named by the box's word.
fn option_name(a: ParamAddr, i: usize) -> &'static str {
    a.spec()
        .and_then(|s| names(s.fmt))
        .and_then(|n| n.get(i).copied())
        .unwrap_or("-")
}

/// MODAL's MODEL, by its option name.
pub fn modal_variant(m: ResonatorMode) -> &'static str {
    option_name(
        ParamAddr::new(BlockRef::Modal, ModalParams::MODE),
        m as usize,
    )
}

/// TAPE's MODEL, by its option name.
pub fn tape_variant(t: TapeMachine) -> &'static str {
    option_name(
        ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODEL),
        usize::from(t.index()),
    )
}

/// A pad's MACHINE, by its option name.
pub fn machine_variant(m: Machine) -> &'static str {
    option_name(ParamAddr::new(BlockRef::Pad, PadParams::MACHINE), m.index())
}

/// One of each panel `env_panel` and friends tell apart.
const ENVS: [EnvKind; 7] = [
    EnvKind::A(EnvSpeed::Fast),
    EnvKind::A(EnvSpeed::Med),
    EnvKind::A(EnvSpeed::Slow),
    EnvKind::B(Func::Env(EnvForm::Ad)),
    EnvKind::B(Func::Lfo(LfoForm::Free)),
    EnvKind::B(Func::Lfo(LfoForm::Lfv)),
    EnvKind::B(Func::Burst(EnvForm::Ad)),
];
const LFOS: [LfoKind; 3] = [
    LfoKind::Classic,
    LfoKind::Func(LfoForm::Free),
    LfoKind::Func(LfoForm::Lfv),
];
const OPS: [OpKind; 3] = [OpKind::Table(WaveId::W1), OpKind::Formant, OpKind::Noise];
/// MODAL's models, in MODEL's order (`ResonatorMode::from_u8`).
pub const MODELS: [ResonatorMode; 4] = [
    ResonatorMode::String,
    ResonatorMode::Modal,
    ResonatorMode::Bowed,
    ResonatorMode::Sympathetic,
];

/// A cell of a panel: a parameter under the cell's label, a matrix route
/// (slice 2's), or TAPE's SOURCE, read-only.
enum Cell {
    Param {
        addr: EdAddr,
        label: &'static str,
        fmt: ValFmt,
    },
    Route(&'static str),
    Source(&'static str),
}

/// `a`'s range: a filter's is its kind's (`FilterParams::spec`), MODE's
/// names and count differ by kind.
pub(super) fn spec_under(a: ParamAddr, ctx: &SlotCtx) -> Option<&'static ParamSpec> {
    let kind = match a.block {
        BlockRef::Filter => ctx.kind,
        BlockRef::PadFilter => ctx.pad_kind,
        _ => return a.spec(),
    };
    let mut f = FilterParams::default();
    f.set_kind(kind);
    Block::spec(&f, a.param)
}

/// What a param's control is.
fn kind_word(fmt: ValFmt) -> &'static str {
    match names(fmt) {
        Some(["OFF", "ON"]) => "toggle",
        Some(_) => "choice",
        None => "cont",
    }
}

/// A choice's names: `Names` is the one format that has them.
pub(super) fn names(fmt: ValFmt) -> Option<&'static [&'static str]> {
    if let ValFmt::Names(n) = fmt {
        Some(n)
    } else {
        None
    }
}

/// How the value moves under a hand: in its unit, or in octaves.
fn scale_word(s: Scale) -> &'static str {
    match s {
        Scale::Linear => "lin",
        Scale::Octaves => "oct",
    }
}

impl Cell {
    fn write(&self, ctx: &SlotCtx, w: &mut dyn Write) -> fmt::Result {
        let (addr, label, fmt) = match *self {
            Cell::Route(label) => return writeln!(w, "route \"{label}\" next-slice"),
            Cell::Source(label) => return writeln!(w, "source \"{label}\""),
            Cell::Param { addr, label, fmt } => (addr, label, fmt),
        };
        let Some(spec) = spec_under(addr.param(), ctx) else {
            return Ok(());
        };
        write!(
            w,
            "param {addr} \"{label}\" {} {}",
            kind_word(fmt),
            scale_word(spec.scale)
        )?;
        for v in [spec.min, spec.max, spec.step, spec.default] {
            w.write_char(' ')?;
            num(w, v)?;
        }
        w.write_char('\n')?;
        for (i, name) in names(fmt).unwrap_or(&[]).iter().enumerate() {
            writeln!(w, "opt {addr} {i} \"{name}\"")?;
        }
        Ok(())
    }
}

/// `v` to seven decimals, trailing zeros dropped. Core's float `Display`
/// is 20 KB of bank 1 this module can't be placed out of.
pub(super) fn num(w: &mut dyn Write, v: f32) -> fmt::Result {
    let mut whole = v.abs() as u32;
    let mut frac = ((v.abs() - whole as f32) * 1e7 + 0.5) as u32;
    if frac >= 10_000_000 {
        whole += 1;
        frac -= 10_000_000;
    }
    if v < 0.0 && (whole, frac) != (0, 0) {
        w.write_char('-')?;
    }
    write!(w, "{whole}")?;
    if frac != 0 {
        // Seven places, zero-padded; the trailing zeros dropped.
        let mut digits = [b'0'; 7];
        let mut n = 7;
        for d in digits.iter_mut().rev() {
            *d += (frac % 10) as u8;
            frac /= 10;
        }
        while digits[n - 1] == b'0' {
            n -= 1;
        }
        w.write_char('.')?;
        for &d in &digits[..n] {
            w.write_char(char::from(d))?;
        }
    }
    Ok(())
}

/// What a cell's binding varies with: the axis a `Group` sets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Fixed,
    Filter,
    Fx,
    Env(EnvSlot),
    Lfo(LfoSlot),
    Op(Op),
    Modal,
    Tape,
    Machine,
    Route,
}

/// The axis a cell bound `b` resolves on; `None` for a cell the editor
/// never shows (a selector, ORBIT's, the mixer's).
fn axis_of(b: SlotBinding) -> Option<Axis> {
    Some(match b {
        SlotBinding::Param(_) | SlotBinding::Source => Axis::Fixed,
        SlotBinding::FilterPanel(_) | SlotBinding::PadFilterPanel(_) => Axis::Filter,
        SlotBinding::FxPanel(_) => Axis::Fx,
        SlotBinding::EnvPanel(s, _) => Axis::Env(s),
        SlotBinding::LfoPanel(s, _) => Axis::Lfo(s),
        SlotBinding::OpPanel(op, _) => Axis::Op(op),
        SlotBinding::ModalPanel(..) => Axis::Modal,
        SlotBinding::Machine(_) => Axis::Tape,
        SlotBinding::Knob(_) => Axis::Machine,
        SlotBinding::SelectedRoute(_) => Axis::Route,
        SlotBinding::Empty
        | SlotBinding::SelectedOp(_)
        | SlotBinding::SelectOp
        | SlotBinding::SelectPad
        | SlotBinding::SelectRoute
        | SlotBinding::Legacy(_)
        | SlotBinding::ReverbPanel(..)
        | SlotBinding::SelectedRing(_)
        | SlotBinding::Orbit(_) => return None,
    })
}

/// A page's panels: what its cells resolve against, one per variant.
#[derive(Clone, Copy)]
enum Group {
    Fixed,
    Filter(FilterKind),
    Fx(FxType),
    Env(EnvSlot, EnvKind),
    Lfo(LfoSlot, LfoKind),
    Op(Op, OpKind),
    Modal(ResonatorMode),
    Tape(TapeMachine),
    Machine(Machine),
    Route(RouteId),
}

/// A pad's panel names start `PAD<n>_`.
fn pad_prefix(pad: Option<PadId>, w: &mut dyn Write) -> fmt::Result {
    match pad {
        Some(p) => write!(w, "PAD{}_", p.number()),
        None => Ok(()),
    }
}

impl Group {
    fn axis(self) -> Axis {
        match self {
            Group::Fixed => Axis::Fixed,
            Group::Filter(_) => Axis::Filter,
            Group::Fx(_) => Axis::Fx,
            Group::Env(s, _) => Axis::Env(s),
            Group::Lfo(s, _) => Axis::Lfo(s),
            Group::Op(o, _) => Axis::Op(o),
            Group::Modal(_) => Axis::Modal,
            Group::Tape(_) => Axis::Tape,
            Group::Machine(_) => Axis::Machine,
            Group::Route(_) => Axis::Route,
        }
    }

    /// `panel <name> <variant>`.
    fn write_head(self, def: &BlockDef, pad: Option<PadId>, w: &mut dyn Write) -> fmt::Result {
        w.write_str("panel ")?;
        match (self, pad) {
            (Group::Fixed, _) => pad_prefix(pad, w).and_then(|()| w.write_str(def.short))?,
            (Group::Filter(_), Some(_)) => pad_prefix(pad, w).and_then(|()| w.write_str("FLT"))?,
            (Group::Filter(_), None) => w.write_str("FILTER")?,
            (Group::Fx(_), _) => w.write_str("FX")?,
            (Group::Env(s, _), _) => w.write_str(["ENV1", "ENV2", "ENV3"][s.index()])?,
            (Group::Lfo(s, _), _) => w.write_str(["LFO1", "LFO2", "LFO3"][s.index()])?,
            (Group::Op(o, _), _) => {
                w.write_str(["OP1", "OP2", "OP3", "OP4", "OP5", "OP6"][o.index()])?
            }
            (Group::Modal(_), _) => w.write_str("RESONATOR")?,
            (Group::Tape(_), _) => w.write_str("MACHINE")?,
            (Group::Machine(_), _) => pad_prefix(pad, w).and_then(|()| w.write_str("MCH"))?,
            (Group::Route(r), _) => {
                pad_prefix(pad, w)?;
                write!(w, "RT{}", r.number())?
            }
        }
        w.write_char(' ')?;
        variant_token(self.variant(), w)?;
        w.write_char('\n')
    }

    fn variant(self) -> &'static str {
        match self {
            Group::Fixed | Group::Route(_) => "-",
            Group::Filter(k) => filter_variant(k),
            Group::Fx(t) => fx_variant(t),
            Group::Env(_, k) => env_variant(k),
            Group::Lfo(_, k) => lfo_variant(k),
            Group::Op(_, k) => op_variant(k),
            Group::Modal(m) => modal_variant(m),
            Group::Tape(t) => tape_variant(t),
            Group::Machine(m) => machine_variant(m),
        }
    }

    /// `base` with this group's axis set to its variant.
    fn ctx(self, base: SlotCtx) -> SlotCtx {
        let mut c = base;
        match self {
            Group::Fixed => {}
            Group::Filter(k) => {
                c.kind = k;
                c.pad_kind = k;
            }
            Group::Fx(t) => c.fx = t,
            Group::Env(s, k) => c.envs[s.index()] = k,
            Group::Lfo(s, k) => c.lfos[s.index()] = k,
            Group::Op(o, k) => c.op_kinds[o.index()] = k,
            Group::Modal(m) => c.model = m,
            Group::Tape(t) => c.tape = t,
            Group::Machine(m) => c.pad = Some(m),
            Group::Route(r) => c.route = r,
        }
        c
    }

    /// Slot `i` of `def` under this group, on pad `pad`'s page.
    fn cell(self, def: &BlockDef, i: usize, ctx: &SlotCtx, pad: Option<PadId>) -> Option<Cell> {
        let slot = def.params.get(i)?;
        if axis_of(slot.binding)? != self.axis() {
            return None;
        }
        match view::view(def, i, ctx) {
            View::Param {
                addr, label, fmt, ..
            } => Some(Cell::Param {
                addr: EdAddr::shown(addr, pad)?,
                label,
                fmt,
            }),
            View::Route { label, .. } => Some(Cell::Route(label)),
            View::Source(_) => Some(Cell::Source(slot.label())),
            View::Empty
            | View::SelectOp
            | View::SelectPad
            | View::SelectRoute
            | View::Legacy { .. }
            | View::Text { .. }
            | View::Orbit(_) => None,
        }
    }
}

/// Group `i`, in the order a page lists them.
fn group_at(mut i: usize) -> Option<Group> {
    if i == 0 {
        return Some(Group::Fixed);
    }
    i -= 1;
    if let Some(&k) = FilterKind::BUILT.get(i) {
        return Some(Group::Filter(k));
    }
    i -= FilterKind::BUILT.len();
    if let Some(&t) = FxType::LIST.get(i) {
        return Some(Group::Fx(t));
    }
    i -= FxType::LIST.len();
    if i < ENVS.len() * EnvSlot::ALL.len() {
        let slot = *EnvSlot::ALL.get(i / ENVS.len())?;
        return Some(Group::Env(slot, *ENVS.get(i % ENVS.len())?));
    }
    i -= ENVS.len() * EnvSlot::ALL.len();
    if i < LFOS.len() * LfoSlot::ALL.len() {
        let slot = *LfoSlot::ALL.get(i / LFOS.len())?;
        return Some(Group::Lfo(slot, *LFOS.get(i % LFOS.len())?));
    }
    i -= LFOS.len() * LfoSlot::ALL.len();
    if i < OPS.len() * Op::ALL.len() {
        let op = *Op::ALL.get(i / OPS.len())?;
        return Some(Group::Op(op, *OPS.get(i % OPS.len())?));
    }
    i -= OPS.len() * Op::ALL.len();
    if let Some(&m) = MODELS.get(i) {
        return Some(Group::Modal(m));
    }
    i -= MODELS.len();
    if let Some(&t) = TapeMachine::ALL.get(i) {
        return Some(Group::Tape(t));
    }
    i -= TapeMachine::ALL.len();
    if let Some(&m) = Machine::ALL.get(i) {
        return Some(Group::Machine(m));
    }
    i -= Machine::ALL.len();
    RouteId::ALL.get(i).map(|&r| Group::Route(r))
}

/// A `BlockRead` with no blocks: the context every param at its zero.
struct NoBlocks;

impl BlockRead for NoBlocks {
    fn block(&self, _: BlockRef) -> Option<BlockView<'_>> {
        None
    }
}

/// The context describe resolves against before a group sets its axis:
/// DRUM's reads the pad's filter.
fn base_ctx(engine: EngineType) -> SlotCtx {
    let c = SlotCtx::read(&NoBlocks, Op::A);
    match engine {
        EngineType::Drum => c.on_drum(),
        EngineType::Algo | EngineType::Modal | EngineType::Tape | EngineType::Va => c,
    }
}

/// `page`'s answer, but for the closing `OK`; `None` past the last page.
pub(super) fn describe(engine: EngineType, page: u8, w: &mut dyn Write) -> Option<fmt::Result> {
    let (def, pad) = page_at(engine, usize::from(page))?;
    Some(write_page(engine, def, pad, page, w))
}

fn write_page(
    engine: EngineType,
    def: &BlockDef,
    pad: Option<PadId>,
    page: u8,
    w: &mut dyn Write,
) -> fmt::Result {
    write!(w, "page {page} ")?;
    pad_prefix(pad, w)?;
    writeln!(w, "{} of {}", def.short, pages(engine))?;
    let base = base_ctx(engine);
    let mut g = 0;
    while let Some(group) = group_at(g) {
        g += 1;
        // A page with a cell on the group's axis lists the panel, cells
        // or none (FX OFF has no pots): the editor knows the variant.
        let mut on = false;
        for s in &def.params {
            on |= axis_of(s.binding) == Some(group.axis());
        }
        if !on {
            continue;
        }
        group.write_head(def, pad, w)?;
        let ctx = group.ctx(base);
        for i in 0..def.params.len() {
            if let Some(c) = group.cell(def, i, &ctx, pad) {
                c.write(&ctx, w)?;
            }
        }
    }
    Ok(())
}

/// Note in `d` every param `describe` lists for `engine`: `write_page`'s
/// walk, to a mask and not a writer.
pub(super) fn mark_described(engine: EngineType, d: &mut super::since::Described) {
    let base = base_ctx(engine);
    let mut page = 0;
    while let Some((def, pad)) = page_at(engine, page) {
        page += 1;
        let mut g = 0;
        while let Some(group) = group_at(g) {
            g += 1;
            let ctx = group.ctx(base);
            for i in 0..def.params.len() {
                if let Some(Cell::Param { addr, .. }) = group.cell(def, i, &ctx, pad) {
                    d.mark(addr);
                }
            }
        }
    }
}
