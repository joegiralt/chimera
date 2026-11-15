//! `ed describe ALGO <page>`: one page's static schema, from the chain's
//! defs and the panel tables, never the Sound. A page lists its panels,
//! each a variant of what a cell resolves against (`FILTER LADDER`,
//! `ENV1 B-LFO`, `OP3 FORMANT`); `-` for cells no variant changes.
//!
//! ```text
//! page <n> <LABEL> of <count>
//! panel <name> <variant>
//! param <addr> "<label>" <cont|choice|toggle> <min> <max> <step> <default>
//! opt <addr> <index> "<name>"
//! route "<cell label>" next-slice
//! ```
//!
//! The chain is `ALGO_FORMANT_CHAIN`: ALGO's with the FORMANT page, so the
//! editor has every page any operator mix can show.

use core::fmt::{self, Write};

use super::EdAddr;
use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::{Block, ParamSpec, ValFmt};
use crate::dsp::algo::kinds::OpKind;
use crate::dsp::algo::waves::WaveId;
use crate::dsp::filter::{FilterKind, KIND_NAMES};
use crate::dsp::modulator::{EnvForm, EnvSlot, EnvSpeed, Func, LfoForm, LfoSlot};
use crate::dsp::voice_fx::params::FxType;
use crate::params::{EngineType, FilterParams};
use crate::ui::block_def::{BlockDef, ParamSlot, SlotBinding};
use crate::ui::block_registry::ALGO_FORMANT_CHAIN;
use crate::ui::filter_panel::{self, PanelTarget};
use crate::ui::view::{EnvKind, LfoKind};
use crate::ui::{fx_panel, mod_panel, op_page};

/// The pages the editor walks for `engine`: 0 for an engine not described.
/// No closures or adapters here or below: one over a `console::ed` type
/// would be instantiated into bank 1, a call back into bank 2.
pub fn pages(engine: EngineType) -> u8 {
    let mut n = 0;
    if engine == EngineType::Algo {
        for b in ALGO_FORMANT_CHAIN.blocks {
            n += 1 + b.sub_pages.len();
        }
    }
    n as u8
}

/// Flat page `n` of the chain: each node's home, then its sub-pages.
fn def_at(mut n: usize) -> Option<&'static BlockDef> {
    for b in ALGO_FORMANT_CHAIN.blocks {
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

/// A panel's variant names are one token; `watch` reports the same ones.
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

/// A cell of a panel: a parameter under the panel's label (and readout,
/// where the panel has its own), or a matrix route (slice 2's).
enum Cell {
    Param {
        block: BlockRef,
        spec: &'static ParamSpec,
        label: &'static str,
        fmt: Option<ValFmt>,
    },
    Route(&'static str),
}

fn param(
    block: BlockRef,
    id: crate::block::ParamId,
    label: &'static str,
    fmt: Option<ValFmt>,
) -> Option<Cell> {
    let spec = ParamAddr::new(block, id).spec()?;
    Some(Cell::Param {
        block,
        spec,
        label,
        fmt,
    })
}

impl Cell {
    fn write(&self, w: &mut dyn Write) -> fmt::Result {
        let (block, spec, label, panel_fmt) = match *self {
            Cell::Route(label) => return writeln!(w, "route \"{label}\" next-slice"),
            Cell::Param {
                block,
                spec,
                label,
                fmt,
            } => (block, spec, label, fmt),
        };
        let addr = EdAddr(ParamAddr::new(block, spec.id));
        let names = match (panel_fmt, spec.fmt) {
            (Some(ValFmt::Names(n)), _) | (_, ValFmt::Names(n)) => Some(n),
            _ => None,
        };
        let kind = match names {
            Some(["OFF", "ON"]) => "toggle",
            Some(_) => "choice",
            None => "cont",
        };
        write!(w, "param {addr} \"{label}\" {kind}")?;
        for v in [spec.min, spec.max, spec.step, spec.default] {
            w.write_char(' ')?;
            num(w, v)?;
        }
        w.write_char('\n')?;
        for (i, name) in names.unwrap_or(&[]).iter().enumerate() {
            writeln!(w, "opt {addr} {i} \"{name}\"")?;
        }
        Ok(())
    }
}

/// `v` to seven decimals, trailing zeros dropped. Core's float `Display`
/// is 20 KB of bank 1 this module can't be placed out of.
fn num(w: &mut dyn Write, v: f32) -> fmt::Result {
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

/// Filter MODE's range and names are the kind's, `FilterParams::spec`'s.
fn filter_spec(kind: FilterKind, id: crate::block::ParamId) -> Option<&'static ParamSpec> {
    let mut f = FilterParams::default();
    f.set_kind(kind);
    Block::spec(&f, id)
}

fn filter_cell(kind: FilterKind, s: &ParamSlot) -> Option<Cell> {
    let SlotBinding::FilterPanel(k) = s.binding else {
        return None;
    };
    let knob = filter_panel::knob(kind, k)?;
    match knob.target {
        PanelTarget::Filter(id) => Some(Cell::Param {
            block: BlockRef::Filter,
            spec: filter_spec(kind, id)?,
            label: knob.label,
            fmt: None,
        }),
        PanelTarget::Route(_) | PanelTarget::Keybd | PanelTarget::JackAmount(_) => {
            Some(Cell::Route(knob.label))
        }
    }
}

fn fx_cell(t: FxType, s: &ParamSlot) -> Option<Cell> {
    let SlotBinding::FxPanel(k) = s.binding else {
        return None;
    };
    let c = fx_panel::cells(t)?.get(usize::from(k))?;
    param(BlockRef::VoiceFx, c.id, c.label, Some(c.fmt))
}

fn slot_cell(p: &mod_panel::ModPanel, block: BlockRef, k: u8) -> Option<Cell> {
    match (*p.slots.get(usize::from(k))?)? {
        mod_panel::PanelSlot::Param { id, label, fmt } => param(block, id, label, Some(fmt)),
        mod_panel::PanelSlot::Fixed { .. } => None,
    }
}

fn op_cell(op: Op, kind: OpKind, k: u8) -> Option<Cell> {
    let (id, label) = op_page::page_cells(kind)[usize::from(k)]?;
    param(BlockRef::AlgoOp(op), id, label, None)
}

fn fixed_cell(s: &ParamSlot) -> Option<Cell> {
    let SlotBinding::Param(a) = s.binding else {
        return None;
    };
    param(
        a.block,
        a.param,
        s.label_override
            .map_or_else(|| a.spec().map_or("", |p| p.label), |l| *l),
        None,
    )
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
}

impl Group {
    fn name(self, def: &BlockDef) -> &'static str {
        match self {
            Group::Fixed => def.short,
            Group::Filter(_) => "FILTER",
            Group::Fx(_) => "FX",
            Group::Env(s, _) => ["ENV1", "ENV2", "ENV3"][s.index()],
            Group::Lfo(s, _) => ["LFO1", "LFO2", "LFO3"][s.index()],
            Group::Op(o, _) => ["OP1", "OP2", "OP3", "OP4", "OP5", "OP6"][o.index()],
        }
    }

    fn variant(self) -> &'static str {
        match self {
            Group::Fixed => "-",
            Group::Filter(k) => filter_variant(k),
            Group::Fx(t) => fx_variant(t),
            Group::Env(_, k) => env_variant(k),
            Group::Lfo(_, k) => lfo_variant(k),
            Group::Op(_, k) => op_variant(k),
        }
    }

    /// Whether the page has a cell this group resolves.
    fn on(self, b: SlotBinding) -> bool {
        match (self, b) {
            (Group::Fixed, SlotBinding::Param(_))
            | (Group::Filter(_), SlotBinding::FilterPanel(_))
            | (Group::Fx(_), SlotBinding::FxPanel(_)) => true,
            (Group::Env(s, _), SlotBinding::EnvPanel(o, _)) => s == o,
            (Group::Lfo(s, _), SlotBinding::LfoPanel(o, _)) => s == o,
            (Group::Op(s, _), SlotBinding::OpPanel(o, _)) => s == o,
            _ => false,
        }
    }

    fn cell(self, s: &ParamSlot) -> Option<Cell> {
        match (self, s.binding) {
            (Group::Fixed, _) => fixed_cell(s),
            (Group::Filter(k), _) => filter_cell(k, s),
            (Group::Fx(t), _) => fx_cell(t, s),
            (Group::Env(sl, k), SlotBinding::EnvPanel(o, i)) if sl == o => {
                slot_cell(mod_panel::env_panel(k), BlockRef::Env(sl), i)
            }
            (Group::Lfo(sl, k), SlotBinding::LfoPanel(o, i)) if sl == o => {
                slot_cell(mod_panel::lfo_panel(k), BlockRef::Lfo(sl), i)
            }
            (Group::Op(op, k), SlotBinding::OpPanel(o, i)) if op == o => op_cell(op, k, i),
            _ => None,
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
    let op = *Op::ALL.get(i / OPS.len())?;
    Some(Group::Op(op, *OPS.get(i % OPS.len())?))
}

/// `page`'s answer, but for the closing `OK`; `None` past the last page.
pub(super) fn describe(engine: EngineType, page: u8, w: &mut dyn Write) -> Option<fmt::Result> {
    if page >= pages(engine) {
        return None;
    }
    Some(write_page(
        def_at(usize::from(page))?,
        page,
        pages(engine),
        w,
    ))
}

fn write_page(def: &BlockDef, page: u8, total: u8, w: &mut dyn Write) -> fmt::Result {
    writeln!(w, "page {page} {} of {total}", def.short)?;
    let mut i = 0;
    while let Some(g) = group_at(i) {
        i += 1;
        let mut shown = false;
        for s in &def.params {
            shown |= g.on(s.binding);
        }
        if !shown {
            continue;
        }
        writeln!(w, "panel {} {}", g.name(def), g.variant())?;
        for s in &def.params {
            if let Some(c) = g.cell(s) {
                c.write(w)?;
            }
        }
    }
    Ok(())
}
