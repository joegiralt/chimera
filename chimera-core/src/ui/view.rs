//! A page slot as it reads now (filter-routing spec § UI): params, the
//! filter kind's panel and route views resolve here against the
//! Sound, so pages, vizzes and encoders all read by address.

use crate::addr::{BlockRead, BlockRef, Op, ParamAddr};
use crate::block::ValFmt;
use crate::dsp::algo::params::AlgoParams;
use crate::dsp::filter::FilterKind;
use crate::dsp::lfo::LfoParams;
use crate::dsp::modal::{self, ModalParams, ResonatorMode};
use crate::dsp::modulator::{
    EnvForm, EnvSlot, EnvSpeed, EnvType, Func, FuncMode, LfoForm, LfoSlot, LfoType, pick,
};
use crate::dsp::tape_engine::machine::{self, Cell};
use crate::dsp::tape_engine::{self, TapeEngineParams as T, TapeModel, wave};
use crate::modulation::{ModSource, VCA};
use crate::orbit::RingIdx;
use crate::orbit::clock::ClockSource;
use crate::orbit::scene::SceneRing;
use crate::params::{EnvParams, FilterParams, OutParams};
use crate::preset::Sound;
use crate::project::Pool;
use crate::storage::SourceRef;
use crate::ui::block_def::{BlockDef, MachineSlot, SlotBinding};
use crate::ui::filter_panel::{self, PanelKnob, PanelTarget};
use crate::ui::fmt::FmtBuf;
use crate::ui::glyph::FocusGlyph;
use crate::ui::hint::{self, Gesture, Hint};
use crate::ui::mod_panel::{self, PanelSlot};
use crate::ui::orbit::OrbitCell;

/// What an ENV slot's page resolves against: type A's panel follows its
/// SPEED, type B's its MODE and that MODE's FORM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvKind {
    A(EnvSpeed),
    B(Func),
}

/// What an LFO slot's page resolves against: CLASSIC, or FUNC and its
/// FORM (FUNC's MODE is always LFO).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoKind {
    Classic,
    Func(LfoForm),
}

/// B's `Func` from its MODE and FORM values (FORM indexes MODE's list).
fn func_at(mode: f32, form: f32) -> Func {
    match pick(&FuncMode::ALL, mode) {
        FuncMode::Env => Func::Env(pick(&EnvForm::ALL, form)),
        FuncMode::Lfo => Func::Lfo(pick(&LfoForm::ALL, form)),
        FuncMode::Burst => Func::Burst(pick(&EnvForm::ALL, form)),
    }
}

/// What a page's panels resolve against (spec § UI "Slot binding").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotCtx {
    pub sel_op: Op,
    pub kind: FilterKind,
    pub envs: [EnvKind; 3],
    pub lfos: [LfoKind; 3],
    /// MODEL: which cells MDL2 shows.
    pub model: ResonatorMode,
    /// TAPE's machine: its labels and its model page's cells.
    pub tape: TapeModel,
    /// TAPE's PERIOD is 0: an unpitched source plays MONO as POLY.
    pub unpitched: bool,
    /// TAPE's SOURCE as SRC reads it.
    pub src: SourceState,
    /// The ORBIT ring the location selects: what `SelectedRing` edits.
    pub ring: RingIdx,
    /// ORBIT's clock: BPM is inert while MIDI leads.
    pub clock: ClockSource,
}

impl SlotCtx {
    /// Read from any `BlockRead`: a Sound's params or a Part view.
    pub fn read(params: &impl BlockRead, sel_op: Op) -> Self {
        let get = |b: BlockRef, id| params.block(b).map_or(0.0, |blk| blk.get(id));
        Self {
            sel_op,
            kind: FilterKind::from_index(get(BlockRef::Filter, FilterParams::KIND)),
            envs: EnvSlot::ALL.map(|s| {
                let at = |id| get(BlockRef::Env(s), id);
                match pick(&EnvType::ALL, at(EnvParams::TYPE)) {
                    EnvType::A => EnvKind::A(pick(&EnvSpeed::ALL, at(EnvParams::SPEED))),
                    EnvType::B => EnvKind::B(func_at(at(EnvParams::MODE), at(EnvParams::FORM))),
                }
            }),
            lfos: LfoSlot::ALL.map(|s| {
                let at = |id| get(BlockRef::Lfo(s), id);
                match pick(&LfoType::ALL, at(LfoParams::TYPE)) {
                    LfoType::Classic => LfoKind::Classic,
                    LfoType::Func => LfoKind::Func(pick(&LfoForm::ALL, at(LfoParams::FORM))),
                }
            }),
            model: ResonatorMode::from_u8(get(BlockRef::Modal, ModalParams::MODE) as u8),
            tape: TapeModel::from_index(get(BlockRef::TapeEngine, T::MODEL) as u8),
            unpitched: get(BlockRef::TapeEngine, T::PERIOD) == 0.0,
            src: SourceState::None,
            ring: RingIdx::ALL[0],
            clock: ClockSource::Internal,
        }
    }

    /// This context with SOURCE read as `src`.
    pub fn on_source(self, src: SourceState) -> Self {
        Self { src, ..self }
    }

    /// The models a page's name follows.
    pub fn models(&self) -> Models {
        Models {
            modal: self.model,
            tape: self.tape,
        }
    }

    /// This context on ORBIT ring `ring`.
    pub fn on_ring(self, ring: RingIdx) -> Self {
        Self { ring, ..self }
    }

    /// This context under ORBIT's `clock`.
    pub fn on_clock(self, clock: ClockSource) -> Self {
        Self { clock, ..self }
    }
}

/// The engines' models a page's name follows: MDL2 MODAL's, the model
/// page TAPE's machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Models {
    pub modal: ResonatorMode,
    pub tape: TapeModel,
}

impl Default for Models {
    fn default() -> Self {
        ResonatorMode::from_u8(0).into()
    }
}

impl From<ResonatorMode> for Models {
    fn from(modal: ResonatorMode) -> Self {
        Models {
            modal,
            tape: TapeModel::Loop,
        }
    }
}

/// TAPE's SOURCE as SRC reads it (spec § 2 refusal, § 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceState {
    /// None picked.
    None,
    /// Picked, but the pool doesn't hold it.
    Missing,
    /// TRANSWAVE on a source that isn't one.
    NotTranswave,
    /// TRON on a zone set (the streamed TRON's).
    ZonesOnTron,
    Held {
        rate: u16,
        frames: u32,
    },
}

impl SourceState {
    /// What a note-on on `model` finds for `r` in `pool`.
    pub fn of(model: TapeModel, r: Option<SourceRef>, pool: &Pool) -> Self {
        let Some(r) = r else {
            return SourceState::None;
        };
        match pool.source_view(r) {
            None => SourceState::Missing,
            Some(v) if wave::playable(model, &v) => SourceState::Held {
                rate: r.rate,
                frames: r.frames,
            },
            Some(_) if model == TapeModel::Transwave => SourceState::NotTranswave,
            Some(_) => SourceState::ZonesOnTron,
        }
    }

    /// The focus band's: `24K · 1.5 S`, or why it won't play.
    pub fn text(self, buf: &mut FmtBuf) {
        use core::fmt::Write;
        let _ = match self {
            SourceState::None => buf.write_str("NONE"),
            SourceState::Missing => buf.write_str("SOURCE MISSING"),
            SourceState::NotTranswave => buf.write_str("NOT A TRANSWAVE"),
            SourceState::ZonesOnTron => buf.write_str("ZONES: NOT TRON"),
            SourceState::Held { rate, .. } => {
                let _ = write!(buf, "{}K · ", rate / 1000);
                self.cell(buf);
                Ok(())
            }
        };
    }

    /// The cell's, short enough for its column: `1.5 S`, `MISSING`.
    pub fn cell(self, buf: &mut FmtBuf) {
        use core::fmt::Write;
        let _ = match self {
            SourceState::None => buf.write_str("NONE"),
            SourceState::Missing => buf.write_str("MISSING"),
            SourceState::NotTranswave => buf.write_str("NO WAVE"),
            SourceState::ZonesOnTron => buf.write_str("ZONES"),
            SourceState::Held { rate, frames } => {
                let rate = u64::from(rate.max(1));
                let tenths = (u64::from(frames) * 10 + rate / 2) / rate;
                write!(buf, "{}.{} S", tenths / 10, tenths % 10)
            }
        };
    }
}

/// A slot, resolved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum View {
    Empty,
    SelectOp,
    Legacy {
        label: &'static str,
        fmt: ValFmt,
    },
    /// A parameter; `dimmed` decides from the Sound whether it is inert.
    Param {
        addr: ParamAddr,
        label: &'static str,
        fmt: ValFmt,
    },
    /// The route `source → CUTOFF` (spec § 6).
    Route {
        source: ModSource,
        label: &'static str,
    },
    /// A fixed readout, dimmed and inert.
    Text {
        label: &'static str,
        text: &'static str,
    },
    /// One of ORBIT's own cells: its text is `orbit::draw::cell_text`.
    Orbit(OrbitCell),
    /// TAPE's SOURCE (`SourceState::text`).
    Source(SourceState),
}

impl View {
    pub fn label(&self) -> &'static str {
        match *self {
            View::Empty => "--",
            View::SelectOp => "OP",
            View::Orbit(c) => c.label(),
            View::Source(_) => "SOURCE",
            View::Legacy { label, .. }
            | View::Param { label, .. }
            | View::Route { label, .. }
            | View::Text { label, .. } => label,
        }
    }

    /// The spec's short form, for a param shown under its own label.
    pub fn short(&self) -> Option<&'static str> {
        match *self {
            View::Param { addr, label, .. } => match addr.spec() {
                Some(s) if s.label == label => s.short,
                // A machine's label carries its own short.
                _ => machine::short_of(addr.param, label),
            },
            _ => None,
        }
    }

    pub fn fmt(&self) -> ValFmt {
        match *self {
            View::Empty | View::Text { .. } => ValFmt::Uni,
            // A readout, not a dial: no bar.
            View::Source(_) => ValFmt::Names(&[]),
            View::SelectOp => ValFmt::OneBased(Op::ALL.len() as u8 - 1),
            View::Legacy { fmt, .. } | View::Param { fmt, .. } => fmt,
            View::Route { .. } => ValFmt::Route,
            View::Orbit(c) => c.fmt(),
        }
    }

    /// The focus glyph: the param's spec's, else ARC; SOURCE none.
    pub fn glyph(&self) -> FocusGlyph {
        if let View::Source(_) = self {
            return FocusGlyph::None;
        }
        self.addr()
            .and_then(|a| a.spec())
            .map_or(FocusGlyph::Arc, |s| s.glyph)
    }

    pub fn addr(&self) -> Option<ParamAddr> {
        match *self {
            View::Param { addr, .. } => Some(addr),
            _ => None,
        }
    }

    /// The gesture hint a param's focus band shows (ADR 0086): MUTE's
    /// MIX + B*n*, which mutes any ring from anywhere in ORBIT. ORBIT's
    /// own cells give theirs through their focus.
    pub fn hint(&self) -> Hint {
        match self.addr() {
            Some(a) if matches!(a.block, BlockRef::OrbitRing(_)) && a.param == SceneRing::MUTE => {
                MUTE_HINT
            }
            _ => hint::NONE,
        }
    }
}

const MUTE_HINT: Hint = &[Gesture("MIX + B1-5", "MUTES THAT RING")];

fn param(addr: ParamAddr, label: &'static str, fmt: ValFmt) -> View {
    View::Param { addr, label, fmt }
}

/// Slot `i` of `def` as it reads under `ctx`.
pub fn view(def: &BlockDef, i: usize, ctx: &SlotCtx) -> View {
    let Some(slot) = def.params.get(i) else {
        return View::Empty;
    };
    match slot.binding {
        SlotBinding::Empty => View::Empty,
        // ORBIT's pages draw and turn their own cells.
        SlotBinding::Orbit(c) => View::Orbit(c),
        SlotBinding::SelectOp => View::SelectOp,
        SlotBinding::Legacy { label, fmt } => View::Legacy { label, fmt },
        // SPD: a type-B slot's SPEED and HOLD read TYPE B (spec § UI).
        SlotBinding::Param(ParamAddr {
            block: BlockRef::Env(s),
            param: EnvParams::SPEED | EnvParams::HOLD_POS,
        }) if matches!(ctx.envs[s.index()], EnvKind::B(_)) => View::Text {
            label: slot.label(),
            text: "TYPE B",
        },
        SlotBinding::Param(addr) => param(addr, slot.label(), slot.format()),
        SlotBinding::SelectedOp(id) => param(
            ParamAddr::new(BlockRef::AlgoOp(ctx.sel_op), id),
            slot.label(),
            slot.format(),
        ),
        SlotBinding::SelectedRing(id) => param(
            ParamAddr::new(BlockRef::OrbitRing(ctx.ring), id),
            slot.label(),
            slot.format(),
        ),
        SlotBinding::FilterPanel(k) => match filter_panel::knob(ctx.kind, k) {
            None => View::Empty,
            Some(&PanelKnob {
                target: PanelTarget::Filter(id),
                label,
            }) => {
                let addr = ParamAddr::new(BlockRef::Filter, id);
                param(addr, label, addr.spec().map_or(ValFmt::Uni, |s| s.fmt))
            }
            Some(&PanelKnob {
                target: PanelTarget::Route(source),
                label,
            }) => View::Route { source, label },
        },
        SlotBinding::EnvPanel(s, k) => panel_view(
            mod_panel::env_panel(ctx.envs[s.index()]),
            k,
            BlockRef::Env(s),
        ),
        SlotBinding::LfoPanel(s, k) => panel_view(
            mod_panel::lfo_panel(ctx.lfos[s.index()]),
            k,
            BlockRef::Lfo(s),
        ),
        SlotBinding::ModalPanel(page, k) => {
            match modal::page_cells(page, ctx.model).get(k as usize) {
                Some(&Some(id)) => {
                    let addr = ParamAddr::new(BlockRef::Modal, id);
                    addr.spec()
                        .map_or(View::Empty, |s| param(addr, s.label, s.fmt))
                }
                _ => View::Empty,
            }
        }
        SlotBinding::Machine(at) => machine_view(at, ctx),
        SlotBinding::Source => View::Source(ctx.src),
    }
}

/// A TAPE cell on the Sound's machine: its label, or the param's own,
/// dimmed by `tape_engine::reads`, where the machine ignores it.
fn machine_view(at: MachineSlot, ctx: &SlotCtx) -> View {
    let k = machine::machine(ctx.tape);
    let (id, cell): (_, Option<&Cell>) = match at {
        MachineSlot::Position => (T::POSITION, k.deck[0].as_ref()),
        MachineSlot::Motion => (T::MOTION, k.deck[1].as_ref()),
        MachineSlot::Page(i) => match k.cells.get(usize::from(i)) {
            Some(c) => (c.id, Some(c)),
            None => return View::Empty,
        },
    };
    let addr = ParamAddr::new(BlockRef::TapeEngine, id);
    let Some(spec) = addr.spec() else {
        return View::Empty;
    };
    let label = cell.map_or(spec.label, |c| c.label);
    if id == T::TYPE && ctx.unpitched {
        return View::Text {
            label,
            text: "POLY (UNPITCHED)",
        };
    }
    param(addr, label, spec.fmt)
}

/// Cell `k` of a modulator panel, its params in `block`.
fn panel_view(panel: &mod_panel::ModPanel, k: u8, block: BlockRef) -> View {
    match panel.slots[k as usize] {
        None => View::Empty,
        Some(PanelSlot::Param { id, label, fmt }) => param(ParamAddr::new(block, id), label, fmt),
        Some(PanelSlot::Fixed { label, text }) => View::Text { label, text },
    }
}

/// A fixed or inapplicable slot draws dimmed, and its encoder is ignored
/// (spec § UI "Dimmed").
pub fn dimmed(addr: ParamAddr, sound: &Sound) -> bool {
    match (addr.block, addr.param) {
        // AMP's VEL under the Algo/Modal pass-through (spec § 5).
        (BlockRef::Out, OutParams::VCA_VEL) => sound.mod_state.routes_into(VCA) == 0,
        // KIND lists built kinds only; with one it is fixed.
        (BlockRef::Filter, FilterParams::KIND) => FilterKind::BUILT.len() == 1,
        // A single-mode kind shows its mode fixed (spec § 7).
        (BlockRef::Filter, FilterParams::MODE) => sound.params.filter.kind().modes().len() == 1,
        // MORPH blends ALG A into ALG B: one algorithm, nothing to blend (#188).
        (BlockRef::Algo, AlgoParams::MORPH) => sound.params.algo.alg_a == sound.params.algo.alg_b,
        // What MODEL ignores (spec § 1 Dimming).
        (BlockRef::Modal, id) => !modal::reads(sound.params.modal.mode, id),
        // What the machine ignores (spec § 3).
        (BlockRef::TapeEngine, id) => !tape_engine::reads(sound.params.tape.model, id),
        _ => false,
    }
}

/// The view is drawn dimmed and inert: a fixed readout, a param `dimmed`
/// rules inapplicable, or BPM while a MIDI clock leads (spec § Design
/// choices: F sets the internal BPM, dimmed under an external clock).
pub fn is_dimmed(v: &View, sound: &Sound, ctx: &SlotCtx) -> bool {
    match *v {
        View::Text { .. } => true,
        View::Param {
            addr:
                ParamAddr {
                    block: BlockRef::Tempo,
                    ..
                },
            ..
        } => ctx.clock == ClockSource::Midi,
        View::Param { addr, .. } => dimmed(addr, sound),
        _ => false,
    }
}
