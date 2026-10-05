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
use crate::dsp::reverb::{ReverbParams, ReverbType, in_plate_zone};
use crate::modulation::{ModSource, VCA};
use crate::orbit::RingIdx;
use crate::orbit::clock::ClockSource;
use crate::orbit::scene::SceneRing;
use crate::params::{EnvParams, FilterParams, OutParams};
use crate::preset::Sound;
use crate::ui::block_def::{BlockDef, SlotBinding};
use crate::ui::filter_panel::{self, PanelKnob, PanelTarget};
use crate::ui::glyph::FocusGlyph;
use crate::ui::hint::{self, Gesture, Hint};
use crate::ui::mod_panel::{self, PanelSlot};
use crate::ui::orbit::OrbitCell;
use crate::ui::reverb_panel;

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
    /// The reverb panel REV and REV › MORE show (`reverb_panel::shown`).
    pub rev: ReverbType,
    /// HALL's SIZE sits in its plate zone: the SIZE cell reads `PLATE`.
    pub plate: bool,
    /// The ORBIT ring the location selects: what `SelectedRing` edits.
    pub ring: RingIdx,
    /// ORBIT's clock: BPM is inert while MIDI leads.
    pub clock: ClockSource,
}

impl SlotCtx {
    /// Read from any `BlockRead`: a Sound's params or a Part view.
    pub fn read(params: &impl BlockRead, sel_op: Op) -> Self {
        let get = |b: BlockRef, id| params.block(b).map_or(0.0, |blk| blk.get(id));
        let rev = ReverbParams {
            kind: ReverbType::at(get(BlockRef::Reverb, ReverbParams::TYPE)),
            size: get(BlockRef::Reverb, ReverbParams::SIZE),
            ..ReverbParams::default()
        };
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
            rev: reverb_panel::shown(rev.kind),
            plate: in_plate_zone(&rev),
            ring: RingIdx::ALL[0],
            clock: ClockSource::Internal,
        }
    }

    /// This context on ORBIT ring `ring`.
    pub fn on_ring(self, ring: RingIdx) -> Self {
        Self { ring, ..self }
    }

    /// This context under reverb `rev` (its TYPE's shown panel, and HALL's
    /// plate zone): a Sound's params hold no FX.
    pub fn on_rev(self, rev: &ReverbParams) -> Self {
        Self {
            rev: reverb_panel::shown(rev.kind),
            plate: in_plate_zone(rev),
            ..self
        }
    }

    /// This context under ORBIT's `clock`.
    pub fn on_clock(self, clock: ClockSource) -> Self {
        Self { clock, ..self }
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
        /// What the cell shows when `label` outruns its bar (ADR 0053).
        short: Option<&'static str>,
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
}

impl View {
    pub fn label(&self) -> &'static str {
        match *self {
            View::Empty => "--",
            View::SelectOp => "OP",
            View::Orbit(c) => c.label(),
            View::Legacy { label, .. }
            | View::Param { label, .. }
            | View::Route { label, .. }
            | View::Text { label, .. } => label,
        }
    }

    /// The cell's short form: the spec's, for a param shown under its own
    /// label; a reverb panel word's own (`DENS`).
    pub fn short(&self) -> Option<&'static str> {
        match *self {
            View::Param { short, .. } => short,
            _ => None,
        }
    }

    pub fn fmt(&self) -> ValFmt {
        match *self {
            View::Empty | View::Text { .. } => ValFmt::Uni,
            View::SelectOp => ValFmt::OneBased(Op::ALL.len() as u8 - 1),
            View::Legacy { fmt, .. } | View::Param { fmt, .. } => fmt,
            View::Route { .. } => ValFmt::Route,
            View::Orbit(c) => c.fmt(),
        }
    }

    /// The focus glyph: the param's spec's, else ARC.
    pub fn glyph(&self) -> FocusGlyph {
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
    let short = addr
        .spec()
        .filter(|s| s.label == label)
        .and_then(|s| s.short);
    View::Param {
        addr,
        label,
        fmt,
        short,
    }
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
        SlotBinding::ReverbPanel(page, k) => match reverb_panel::knob(ctx.rev, page, k) {
            Some(c) => {
                let addr = ParamAddr::new(BlockRef::Reverb, c.id);
                let fmt = addr.spec().map_or(ValFmt::Uni, |s| s.fmt);
                let label = if ctx.plate && c.id == ReverbParams::SIZE {
                    reverb_panel::PLATE
                } else {
                    c.label
                };
                // A panel word's own short (`DENS`) over the spec's.
                match (param(addr, label, fmt), c.short) {
                    (
                        View::Param {
                            addr, label, fmt, ..
                        },
                        Some(s),
                    ) => View::Param {
                        addr,
                        label,
                        fmt,
                        short: Some(s),
                    },
                    (v, _) => v,
                }
            }
            None => View::Empty,
        },
    }
}

/// Whether slot `i` of `def` is a reverb panel cell: its text and step
/// are the TYPE's (`reverb_panel`).
pub fn is_reverb_panel(def: &BlockDef, i: usize) -> bool {
    matches!(
        def.params.get(i).map(|s| s.binding),
        Some(SlotBinding::ReverbPanel(..))
    )
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
