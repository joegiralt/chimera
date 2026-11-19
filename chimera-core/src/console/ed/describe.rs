//! `ed describe <SCHEMA> <page>`: one page's static schema, from the
//! schema's chain and the box's own cell resolver (`view::view`), never the
//! Sound. A page lists its panels, each a variant of what a cell resolves
//! against (`FILTER LADDER`, `ENV1 B-LFO`, `OP3 FORMANT`, `PAD3_MCH TOM`,
//! `REVERB HALL`); `-` for cells no variant changes.
//!
//! ```text
//! page <n> <LABEL> of <count>
//! panel <name> <variant>
//! param <addr> "<label>" <cont|choice|toggle> <lin|oct> <min> <max> <step> <default> <flags>
//! opt <addr> <index> "<name>"
//! route "<cell label>" <cell> <route|keybd>
//! source "<cell label>"
//! ```
//!
//! `flags` is `m` (a MIDI map may target it), `d` (a mod destination),
//! both, or `-` (ADR 0289).
//!
//! ALGO's chain is `ALGO_FORMANT_CHAIN`, so the editor has every page any
//! operator mix can show. On DRUM a page that edits the pad is described
//! once a pad (`PAD3_KIT`), on the mixer one that edits the Part once a
//! Part (`PART3_PRT`), after the shared pages (ADR 0285, 0289).

use core::fmt::{self, Write};

use super::EdAddr;
use super::lit::lit;
use super::schema::{Round, Schema};
use crate::addr::{BlockRef, Op, ParamAddr};
use crate::block::{Block, ParamSpec, Scale, ValFmt};
use crate::dsp::algo::kinds::OpKind;
use crate::dsp::algo::waves::WaveId;
use crate::dsp::drum::sound::RouteId;
use crate::dsp::drum::types::{Machine, PadId};
use crate::dsp::filter::{FilterKind, KIND_NAMES};
use crate::dsp::modal::ResonatorMode;
use crate::dsp::modulator::{EnvForm, EnvSlot, EnvSpeed, Func, LfoForm, LfoSlot};
use crate::dsp::reverb::ReverbType;
use crate::dsp::tape_engine::params::{TapeEngineParams, TapeMachine};
use crate::dsp::voice_fx::params::FxType;
use crate::params::{EngineType, FilterParams};
use crate::ui::block_def::{BlockDef, SlotBinding};
use crate::ui::reverb_panel;
use crate::ui::view::{self, EnvKind, LfoKind, SlotCtx, View};
use chimera_engine::addr::{BlockRead, BlockView};
use chimera_engine::dsp::drum::params::PadParams;
use chimera_engine::dsp::modal::ModalParams;
use chimera_engine::midi_map::Mappable;

use super::pages::{Pages, page_at, schema_pages};
pub use super::pages::{chain, pages};

/// A variant as one token: a name's spaces as `-` (`TONE KICK` is
/// `TONE-KICK`). `since` writes them the same way.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.variant_token")
)]
pub(super) fn variant_token(name: &str, w: &mut dyn Write) -> fmt::Result {
    for c in name.chars() {
        w.write_char(if c == ' ' { '-' } else { c })?;
    }
    Ok(())
}

/// A panel's variant names; `since` reports the same ones.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.filter_variant")
)]
pub fn filter_variant(k: FilterKind) -> &'static str {
    let i = FilterKind::BUILT.iter().position(|&b| b == k).unwrap_or(0);
    KIND_NAMES[i]
}

#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.fx_variant")
)]
pub fn fx_variant(t: FxType) -> &'static str {
    let i = FxType::LIST.iter().position(|&b| b == t).unwrap_or(0);
    FxType::NAMES[i]
}

/// A reverb TYPE's panel, by its option name.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.reverb_variant")
)]
pub fn reverb_variant(t: ReverbType) -> &'static str {
    ReverbType::NAMES.get(t.index()).copied().unwrap_or("-")
}

/// One name for each panel `mod_panel::env_panel` has.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.env_variant")
)]
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

#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.lfo_variant")
)]
pub fn lfo_variant(k: LfoKind) -> &'static str {
    match k {
        LfoKind::Classic => "CLASSIC",
        LfoKind::Func(LfoForm::Lfv) => "FUNC-LFV",
        LfoKind::Func(_) => "FUNC",
    }
}

#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.op_variant")
)]
pub fn op_variant(k: OpKind) -> &'static str {
    match k {
        OpKind::Table(_) => "TABLE",
        OpKind::Formant => "FORMANT",
        OpKind::Noise => "NOISE",
    }
}

/// Option `i`'s name of `a`'s choice: a variant named by the box's word.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.option_name")
)]
fn option_name(a: ParamAddr, i: usize) -> &'static str {
    a.spec()
        .and_then(|s| names(s.fmt))
        .and_then(|n| n.get(i).copied())
        .unwrap_or("-")
}

/// MODAL's MODEL, by its option name.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.modal_variant")
)]
pub fn modal_variant(m: ResonatorMode) -> &'static str {
    option_name(
        ParamAddr::new(BlockRef::Modal, ModalParams::MODE),
        m as usize,
    )
}

/// TAPE's MODEL, by its option name.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.tape_variant")
)]
pub fn tape_variant(t: TapeMachine) -> &'static str {
    option_name(
        ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODEL),
        usize::from(t.index()),
    )
}

/// A pad's MACHINE, by its option name.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.machine_variant")
)]
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

/// A cell of a panel: a parameter under the cell's label, a FLT › MOD
/// route knob (the matrix cell it views, ADR 0289), or TAPE's SOURCE,
/// read-only.
enum Cell {
    Param {
        addr: EdAddr,
        label: &'static str,
        fmt: ValFmt,
    },
    Route(&'static str, crate::ui::filter_panel::RouteView),
    Source(&'static str),
}

/// `a`'s range: a filter's is its kind's (`FilterParams::spec`), MODE's
/// names and count differ by kind.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.spec_under")
)]
pub(super) fn spec_under(a: ParamAddr, ctx: &SlotCtx) -> Option<&'static ParamSpec> {
    let kind = match a.block {
        BlockRef::Filter => ctx.kind,
        BlockRef::PadFilter => ctx.pad_kind,
        BlockRef::Modal
        | BlockRef::Algo
        | BlockRef::AlgoOp(_)
        | BlockRef::Env(_)
        | BlockRef::Lfo(_)
        | BlockRef::Out
        | BlockRef::Pitch
        | BlockRef::Chorus
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
        | BlockRef::Random
        | BlockRef::TapeEngine
        | BlockRef::Va
        | BlockRef::VoiceFx
        | BlockRef::Pad
        | BlockRef::Kit
        | BlockRef::PadAmp
        | BlockRef::PadEnv
        | BlockRef::PadLfo
        | BlockRef::PadRoute(_) => return a.spec(),
    };
    let mut f = FilterParams::default();
    f.set_kind(kind);
    Block::spec(&f, a.param)
}

/// What a param's control is.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.kind_word")
)]
fn kind_word(fmt: ValFmt) -> &'static str {
    match names(fmt) {
        Some(["OFF", "ON"]) => "toggle",
        Some(_) => "choice",
        None => "cont",
    }
}

/// A choice's names: `Names` is the one format that has them.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.describe.names"))]
pub(super) fn names(fmt: ValFmt) -> Option<&'static [&'static str]> {
    if let ValFmt::Names(n) = fmt {
        Some(n)
    } else {
        None
    }
}

/// How the value moves under a hand: in its unit, or in octaves.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.scale_word")
)]
fn scale_word(s: Scale) -> &'static str {
    match s {
        Scale::Linear => "lin",
        Scale::Octaves => "oct",
    }
}

impl Cell {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.describe.write"))]
    fn write(&self, ctx: &SlotCtx, w: &mut dyn Write) -> fmt::Result {
        let (addr, label, fmt) = match *self {
            Cell::Route(label, r) => return super::route::knob_line(label, r, w),
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
        w.write_char(' ')?;
        flags(addr.param(), w)?;
        w.write_char('\n')?;
        for (i, name) in names(fmt).unwrap_or(&[]).iter().enumerate() {
            writeln!(w, "opt {addr} {i} \"{name}\"")?;
        }
        Ok(())
    }
}

/// What else may target `a`: `m` a MIDI map (`Mappable`), `d` the mod
/// matrix (`modulatable`), `-` neither. Read from the box's own rules, so
/// MIDI MAP's and MTX's pickers need no list of their own.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.describe.flags"))]
fn flags(a: ParamAddr, w: &mut dyn Write) -> fmt::Result {
    let m = Mappable::new(a).is_some();
    let d = a.modulatable();
    if m {
        w.write_char('m')?;
    }
    if d {
        w.write_char('d')?;
    }
    if !m && !d {
        w.write_char('-')?;
    }
    Ok(())
}

/// `v` to seven decimals, trailing zeros dropped. Core's float `Display`
/// is 20 KB of bank 1 this module can't be placed out of.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.describe.num"))]
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
    Reverb,
}

/// The axis a cell bound `b` resolves on; `None` for a cell the editor
/// never shows (a selector, ORBIT's).
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.axis_of")
)]
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
        SlotBinding::ReverbPanel(..) => Axis::Reverb,
        SlotBinding::Empty
        | SlotBinding::SelectedOp(_)
        | SlotBinding::SelectOp
        | SlotBinding::SelectPad
        | SlotBinding::SelectRoute
        | SlotBinding::Legacy(_)
        | SlotBinding::SelectedRing(_)
        | SlotBinding::Orbit(_) => return None,
    })
}

lit!(
    /// The reverb's panel's name.
    REVERB = "REVERB"
);

/// A panel's name, as `describe` lists it and `since` reports its
/// variant: one table for both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PanelName {
    Filter,
    Fx,
    Env(EnvSlot),
    Lfo(LfoSlot),
    Op(Op),
    Resonator,
    Machine,
    PadFilter(PadId),
    PadMachine(PadId),
    PadRoute(PadId, RouteId),
    Reverb,
    /// A page's own cells, a round's when it is one.
    Page(&'static str, Round),
}

impl PanelName {
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.describe.write.1")
    )]
    pub(super) fn write(self, w: &mut dyn Write) -> fmt::Result {
        let (round, name) = match self {
            PanelName::Filter => (Round::Shared, "FILTER"),
            PanelName::Fx => (Round::Shared, "FX"),
            PanelName::Env(s) => (Round::Shared, env_name(s)),
            PanelName::Lfo(s) => (Round::Shared, lfo_name(s)),
            PanelName::Op(o) => (Round::Shared, op_name(o)),
            PanelName::Resonator => (Round::Shared, "RESONATOR"),
            PanelName::Machine => (Round::Shared, "MACHINE"),
            PanelName::PadFilter(p) => (Round::Pad(p), "FLT"),
            PanelName::PadMachine(p) => (Round::Pad(p), "MCH"),
            PanelName::PadRoute(p, r) => (Round::Pad(p), route_name(r)),
            PanelName::Reverb => (Round::Shared, REVERB.get()),
            PanelName::Page(short, r) => (r, short),
        };
        round.prefix(w)?;
        w.write_str(name)
    }
}

#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.env_name")
)]
const fn env_name(s: EnvSlot) -> &'static str {
    match s {
        EnvSlot::Env1 => "ENV1",
        EnvSlot::Env2 => "ENV2",
        EnvSlot::Env3 => "ENV3",
    }
}

#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.lfo_name")
)]
const fn lfo_name(s: LfoSlot) -> &'static str {
    match s {
        LfoSlot::Lfo1 => "LFO1",
        LfoSlot::Lfo2 => "LFO2",
        LfoSlot::Lfo3 => "LFO3",
    }
}

#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.op_name")
)]
const fn op_name(o: Op) -> &'static str {
    match o {
        Op::A => "OP1",
        Op::B => "OP2",
        Op::C => "OP3",
        Op::D => "OP4",
        Op::E => "OP5",
        Op::F => "OP6",
    }
}

/// `RT1`–`RT4`: a route is a number, 1–4.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.route_name")
)]
fn route_name(r: RouteId) -> &'static str {
    ["RT1", "RT2", "RT3", "RT4"]
        .get(r.index())
        .copied()
        .unwrap_or("RT")
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
    Reverb(ReverbType),
}

impl Group {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.describe.axis"))]
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
            Group::Reverb(_) => Axis::Reverb,
        }
    }

    /// `panel <name> <variant>`.
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.describe.write_head")
    )]
    fn write_head(self, name: PanelName, w: &mut dyn Write) -> fmt::Result {
        w.write_str("panel ")?;
        name.write(w)?;
        w.write_char(' ')?;
        variant_token(self.variant(), w)?;
        w.write_char('\n')
    }

    /// Its panel's name on `def`, a page of round `r`; `None` for a pad's
    /// group off a pad's page (no chain has one).
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.describe.name"))]
    fn name(self, def: &'static BlockDef, r: Round) -> Option<PanelName> {
        Some(match (self, r.pad()) {
            (Group::Fixed, _) => PanelName::Page(def.short, r),
            (Group::Filter(_), Some(p)) => PanelName::PadFilter(p),
            (Group::Filter(_), None) => PanelName::Filter,
            (Group::Fx(_), _) => PanelName::Fx,
            (Group::Env(s, _), _) => PanelName::Env(s),
            (Group::Lfo(s, _), _) => PanelName::Lfo(s),
            (Group::Op(o, _), _) => PanelName::Op(o),
            (Group::Modal(_), _) => PanelName::Resonator,
            (Group::Tape(_), _) => PanelName::Machine,
            (Group::Machine(_), Some(p)) => PanelName::PadMachine(p),
            (Group::Route(rt), Some(p)) => PanelName::PadRoute(p, rt),
            (Group::Machine(_) | Group::Route(_), None) => return None,
            (Group::Reverb(_), _) => PanelName::Reverb,
        })
    }

    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.describe.variant")
    )]
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
            Group::Reverb(t) => reverb_variant(t),
        }
    }

    /// `base` with this group's axis set to its variant.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.describe.ctx"))]
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
            Group::Reverb(t) => c.rev = t,
        }
        c
    }

    /// Slot `i` of `def` under this group, on a page of round `r`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.describe.cell"))]
    fn cell(self, def: &BlockDef, i: usize, ctx: &SlotCtx, r: Round) -> Option<Cell> {
        let slot = def.params.get(i)?;
        if axis_of(slot.binding)? != self.axis() {
            return None;
        }
        match view::view(def, i, ctx) {
            View::Param {
                addr, label, fmt, ..
            } => Some(Cell::Param {
                addr: EdAddr::shown(addr, r)?,
                label,
                fmt,
            }),
            View::Route { label, route } => Some(Cell::Route(label, route)),
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
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.group_at")
)]
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
    if let Some(&r) = RouteId::ALL.get(i) {
        return Some(Group::Route(r));
    }
    i -= RouteId::ALL.len();
    reverb_panel::types_on_screen()
        .get(i)
        .map(|&t| Group::Reverb(t))
}

/// A `BlockRead` with no blocks: the context every param at its zero.
struct NoBlocks;

impl BlockRead for NoBlocks {
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.describe.block"))]
    fn block(&self, _: BlockRef) -> Option<BlockView<'_>> {
        None
    }
}

/// The context describe resolves against before a group sets its axis:
/// DRUM's reads the pad's filter.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.base_ctx")
)]
fn base_ctx(s: Schema) -> SlotCtx {
    let c = SlotCtx::read(&NoBlocks, Op::A);
    match s {
        Schema::Engine(EngineType::Drum) => c.on_drum(),
        Schema::Engine(
            EngineType::Algo | EngineType::Modal | EngineType::Tape | EngineType::Va,
        )
        | Schema::Mixer
        | Schema::Matrix(_) => c,
    }
}

/// `page`'s answer, but for the closing `OK`; `None` past the last page.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.describe")
)]
pub(super) fn describe(s: Schema, page: u8, w: &mut dyn Write) -> Option<fmt::Result> {
    if let Schema::Matrix(e) = s {
        return (page == 0).then(|| super::route::describe(e, w));
    }
    let (def, round) = page_at(s, usize::from(page))?;
    Some(write_page(s, def, round, page, w))
}

/// Every page of `s`, in order, into `w`: what `ed schema` hashes, so the
/// hash is describe's own bytes.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.describe_all")
)]
pub(super) fn describe_all(s: Schema, w: &mut dyn Write) -> fmt::Result {
    if let Schema::Matrix(e) = s {
        return super::route::describe(e, w);
    }
    for (n, (def, round)) in Pages::all(s).enumerate() {
        write_page(s, def, round, u8::try_from(n).map_err(|_| fmt::Error)?, w)?;
    }
    Ok(())
}

#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.write_page")
)]
fn write_page(
    s: Schema,
    def: &'static BlockDef,
    round: Round,
    page: u8,
    w: &mut dyn Write,
) -> fmt::Result {
    write!(w, "page {page} ")?;
    round.prefix(w)?;
    writeln!(w, "{} of {}", def.short, schema_pages(s))?;
    let base = base_ctx(s);
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
        let Some(name) = group.name(def, round) else {
            continue;
        };
        group.write_head(name, w)?;
        let ctx = group.ctx(base);
        for i in 0..def.params.len() {
            if let Some(c) = group.cell(def, i, &ctx, round) {
                c.write(&ctx, w)?;
            }
        }
    }
    Ok(())
}

/// Note in `d` every param `describe` lists for `s`: `write_page`'s
/// walk, to a mask and not a writer.
#[cfg_attr(
    target_os = "none",
    unsafe(link_section = ".bank2_ed.describe.mark_described")
)]
pub(super) fn mark_described(s: Schema, d: &mut super::since::Described) {
    let base = base_ctx(s);
    for (def, round) in Pages::all(s) {
        let mut g = 0;
        while let Some(group) = group_at(g) {
            g += 1;
            let ctx = group.ctx(base);
            for i in 0..def.params.len() {
                if let Some(Cell::Param { addr, .. }) = group.cell(def, i, &ctx, round) {
                    d.mark(addr);
                }
            }
        }
    }
}
