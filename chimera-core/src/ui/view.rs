//! A page slot as it reads now (filter-routing spec § UI): params, the
//! filter kind's panel and route views resolve here against the
//! Sound, so pages, vizzes and encoders all read by address.

use crate::addr::{BlockRead, BlockRef, Op, ParamAddr};
use crate::block::ValFmt;
use crate::dsp::algo::kinds::{self, OpKind};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::algo::plan::OPS;
use crate::dsp::chorus::{ChorusMode, ChorusParams, MODE_NAMES};
use crate::dsp::filter::FilterKind;
use crate::dsp::lfo::LfoParams;
use crate::dsp::modal::{self, ModalParams, ResonatorMode};
use crate::dsp::modulator::{
    EnvForm, EnvSlot, EnvSpeed, EnvType, Func, FuncMode, LfoForm, LfoSlot, LfoType, pick,
};
use crate::dsp::reverb::{ReverbParams, ReverbType, in_plate_zone};
use crate::dsp::tape_engine::machine::{self, Cell};
use crate::dsp::tape_engine::{self, TapeEngineParams as T, TapeModel, wave};
use crate::dsp::voice_fx::params::{FxType, VoiceFxParams};
use crate::midi_map::Mappable;
use crate::modulation::VCA;
use crate::name::SoundName;
use crate::orbit::RingIdx;
use crate::orbit::clock::ClockSource;
use crate::orbit::scene::SceneRing;
use crate::params::{EnvParams, FilterParams, OutParams};
use crate::preset::Sound;
use crate::project::arena::Cost;
use crate::project::{Absence, Pool};
use crate::storage::SourceRef;
use crate::ui::block_def::{BlockDef, MachineSlot, SlotBinding};
use crate::ui::filter_panel::{self, PanelKnob, PanelTarget, RouteView};
use crate::ui::fmt::FmtBuf;
use crate::ui::fx_panel;
use crate::ui::glyph::FocusGlyph;
use crate::ui::hint::{self, Gesture, Hint};
use crate::ui::mod_panel::{self, PanelSlot};
use crate::ui::op_page;
use crate::ui::orbit::OrbitCell;
use crate::ui::reverb_panel;
use crate::ui::theme_settings::Pages;

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
    /// The FX TYPE: which pots FLT › FX shows.
    pub fx: FxType,
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
    /// The chorus MODE the CHORUS send hints.
    pub chorus: ChorusMode,
    /// The reverb panel REV and REV › MORE show (`reverb_panel::shown`).
    pub rev: ReverbType,
    /// The delay TYPE whose cells DLY › CHAR shows.
    pub delay: crate::dsp::delay::DelayType,
    /// HALL's SIZE sits in its plate zone: the SIZE cell reads `PLATE`.
    pub plate: bool,
    /// The ORBIT ring the location selects: what `SelectedRing` edits.
    pub ring: RingIdx,
    /// ORBIT's clock: BPM is inert while MIDI leads.
    pub clock: ClockSource,
    /// Each operator's kind: which cells its OP page shows.
    pub op_kinds: [OpKind; OPS],
    /// THEME's PAGES: which key a hint names for the next sub-page.
    pub pages: Pages,
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
            chorus: ChorusMode::from_u8(get(BlockRef::Chorus, ChorusParams::MODE) as u8),
            kind: FilterKind::from_index(get(BlockRef::Filter, FilterParams::KIND)),
            fx: FxType::from_index(get(BlockRef::VoiceFx, VoiceFxParams::TYPE)),
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
            rev: reverb_panel::shown(rev.kind),
            delay: crate::dsp::delay::DelayType::at(get(
                BlockRef::Delay,
                crate::dsp::delay::DelayParams::TYPE,
            )),
            plate: in_plate_zone(&rev),
            ring: RingIdx::ALL[0],
            clock: ClockSource::Internal,
            op_kinds: Op::ALL
                .map(|op| OpKind::clamped(get(BlockRef::AlgoOp(op), AlgoOpParams::WAVE) as u8)),
            pages: crate::ui::theme_settings::ThemeSettings::DEFAULT.pages,
        }
    }

    /// This context under THEME's PAGES.
    pub fn on_pages(self, pages: Pages) -> Self {
        Self { pages, ..self }
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

    /// This context under reverb `rev` (its TYPE's shown panel, and HALL's
    /// plate zone): a Sound's params hold no FX.
    pub fn on_rev(self, rev: &ReverbParams) -> Self {
        Self {
            rev: reverb_panel::shown(rev.kind),
            plate: in_plate_zone(rev),
            ..self
        }
    }

    /// This context under the project chorus `fx` (a Sound's params hold
    /// no FX): the mode the CHORUS send hints.
    pub fn on_chorus(self, fx: &ChorusParams) -> Self {
        Self {
            chorus: ChorusMode::from_u8(fx.mode),
            ..self
        }
    }

    /// This context under the project delay `fx`: the TYPE DLY › CHAR shows.
    pub fn on_delay(self, fx: &crate::dsp::delay::DelayParams) -> Self {
        Self {
            delay: fx.kind,
            ..self
        }
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
    /// Picked, and the pool keeps its ref without its bytes, for this
    /// reason (#372).
    Absent(Absence),
    /// TRANSWAVE on a source that isn't one.
    NotTranswave,
    /// TRON on a zone set (the streamed TRON's).
    ZonesOnTron,
    Held {
        rate: u16,
        frames: u32,
        cost: Cost,
        name: Option<SoundName>,
    },
    /// The cell on a card row, not loaded: its head once read (`None`, its
    /// stem stands in), its cost, and the pool's free bytes.
    Card {
        stem: u32,
        head: Option<(SoundName, u16, u32)>,
        cost: Cost,
        free: Cost,
    },
    /// A card read failed: until the next input.
    Failed(&'static str),
}

impl SourceState {
    /// What a note-on on `model` finds for `r` in `pool`.
    pub fn of(model: TapeModel, r: Option<SourceRef>, pool: &Pool) -> Self {
        let Some(r) = r else {
            return SourceState::None;
        };
        let Some(id) = pool.find_source(r) else {
            return SourceState::Absent(Absence::Missing);
        };
        if let Some(why) = pool.absence(id) {
            return SourceState::Absent(why);
        }
        match pool.source_view(r) {
            Some(v) if wave::playable(model, &v) => SourceState::Held {
                rate: r.rate,
                frames: r.frames,
                cost: pool.source(id).map_or(Cost::default(), |e| e.cost()),
                name: None,
            },
            Some(_) if model == TapeModel::Transwave => SourceState::NotTranswave,
            _ => SourceState::ZonesOnTron,
        }
    }

    /// A held source's name, once its head is known.
    pub fn named(self, n: Option<SoundName>) -> Self {
        match self {
            SourceState::Held {
                rate, frames, cost, ..
            } => SourceState::Held {
                rate,
                frames,
                cost,
                name: n,
            },
            s => s,
        }
    }

    /// The focus band's: `CHOIR · 24K · 1.5 S · 41K`, or why it won't play.
    pub fn text(self, buf: &mut FmtBuf) {
        use core::fmt::Write;
        let tail =
            |buf: &mut FmtBuf, name: Option<SoundName>, rf: Option<(u16, u32)>, cost: Cost| {
                if let Some(n) = name {
                    let _ = write!(buf, "{} · ", n.as_str());
                }
                if let Some((rate, frames)) = rf {
                    let _ = write!(buf, "{}K · ", rate / 1000);
                    seconds(buf, rate, frames);
                    let _ = buf.write_str(" · ");
                }
                let _ = buf.write_str(cost.kib_label().as_str());
            };
        match self {
            SourceState::Held {
                rate,
                frames,
                cost,
                name,
            } => tail(buf, name, Some((rate, frames)), cost),
            SourceState::Card {
                stem,
                head: None,
                cost,
                ..
            } => {
                let _ = write!(buf, "{stem:08X} · ");
                tail(buf, None, None, cost)
            }
            SourceState::Card {
                head: Some((n, rate, frames)),
                cost,
                ..
            } => tail(buf, Some(n), Some((rate, frames)), cost),
            s => {
                let _ = buf.write_str(s.why());
            }
        }
    }

    /// Why it won't play, the band's words.
    fn why(self) -> &'static str {
        use crate::ui::source_cell::{NOT_ON_CARD, READ_FAILED, SOURCE_MISSING};
        match self {
            SourceState::Absent(Absence::Missing) => SOURCE_MISSING,
            SourceState::Absent(Absence::NotOnCard) => NOT_ON_CARD,
            SourceState::Absent(Absence::ReadFailed) => READ_FAILED,
            SourceState::NotTranswave => "NOT A TRANSWAVE",
            SourceState::ZonesOnTron => "ZONES: NOT TRON",
            SourceState::Failed(w) => w,
            _ => "NONE",
        }
    }

    /// The cell's, short enough for its column: `1.5 S`, `MISSING`, a card
    /// row's cost.
    pub fn cell(self, buf: &mut FmtBuf) {
        use core::fmt::Write;
        let _ = match self {
            SourceState::None => buf.write_str("NONE"),
            SourceState::Absent(Absence::Missing) => buf.write_str("MISSING"),
            SourceState::Absent(Absence::NotOnCard) => buf.write_str("NO FILE"),
            SourceState::Absent(Absence::ReadFailed) | SourceState::Failed(_) => {
                buf.write_str("ERROR")
            }
            SourceState::NotTranswave => buf.write_str("NO WAVE"),
            SourceState::ZonesOnTron => buf.write_str("ZONES"),
            SourceState::Held { rate, frames, .. } => {
                seconds(buf, rate, frames);
                Ok(())
            }
            SourceState::Card { cost, .. } => buf.write_str(cost.kib_label().as_str()),
        };
    }

    /// What the focus band and the cell are keyed on: their text, and
    /// whether a card row fits.
    pub fn key(self) -> u32 {
        let (mut b, mut c) = (FmtBuf::new(), FmtBuf::new());
        self.text(&mut b);
        self.cell(&mut c);
        let fits = matches!(self, SourceState::Card { cost, free, .. } if cost <= free);
        b.as_str()
            .bytes()
            .chain(c.as_str().bytes())
            .fold(0x811C_9DC5_u32 ^ fits as u32, |h, x| {
                (h ^ u32::from(x)).wrapping_mul(0x0100_0193)
            })
    }
}

/// `frames` at `rate` in seconds, to a tenth: `1.5 S`.
fn seconds(buf: &mut FmtBuf, rate: u16, frames: u32) {
    let rate = u64::from(rate.max(1));
    let tenths = (u64::from(frames) * 10 + rate / 2) / rate;
    let _ = write!(buf, "{}.{} S", tenths / 10, tenths % 10);
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
    /// A matrix cell's amount (filter-types spec § 2): FM1, FM2, FM3, QM,
    /// ENV AMT, KEYBD.
    Route {
        route: RouteView,
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
            // A readout, not a dial: no bar.
            View::Source(_) => ValFmt::Names(&[]),
            View::SelectOp => ValFmt::OneBased(Op::ALL.len() as u8 - 1),
            View::Legacy { fmt, .. } | View::Param { fmt, .. } => fmt,
            View::Route { route, .. } => route.fmt(),
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

    /// A value a push can set or fine-tune (ADR 0140).
    pub fn pushes(&self) -> bool {
        matches!(self, View::Param { .. } | View::Route { .. })
    }

    pub fn addr(&self) -> Option<ParamAddr> {
        match *self {
            View::Param { addr, .. } => Some(addr),
            _ => None,
        }
    }

    /// The gesture hint a param's focus band shows (ADR 0086): MUTE's
    /// MIX + B*n*, which mutes any ring from anywhere in ORBIT. ORBIT's
    /// own cells give theirs through their focus. A route view's: MIX +
    /// MINUS deletes its route. A mappable param's adds MIX + SEQ's learn.
    pub fn hint(&self, ctx: &SlotCtx) -> Hint {
        if let View::Route { .. } = self {
            return crate::ui::mod_grid::DELETE;
        }
        match self.addr() {
            Some(ParamAddr {
                block: BlockRef::Out,
                param,
            }) if param == OutParams::SEND3 => REV_HINTS[ctx.rev.index()],
            Some(ParamAddr {
                block: BlockRef::Out,
                param,
            }) if param == OutParams::SEND1 => CHORUS_HINTS[ctx.chorus as usize],
            Some(a) if matches!(a.block, BlockRef::OrbitRing(_)) && a.param == SceneRing::MUTE => {
                MUTE_HINT
            }
            Some(a) if cfg!(feature = "midi-map") && Mappable::new(a).is_some() => LEARN_HINT,
            _ => hint::NONE,
        }
    }
}

/// MIX + SEQ learns a CC on the cell (MIDI map spec § 4): every mappable
/// cell's hint, but where the cell's own gesture leaves no room on the line.
pub const LEARN: Gesture<'static> = Gesture("MIX + SEQ", "LEARNS A CC");
/// Every mappable cell's hint; PUSH ENCODERS' replaces it when on.
pub const LEARN_HINT: Hint = &[LEARN];

/// MIX + B*n* in the mixer keeps the page and opens Part *n*'s (ADR 0044).
const PART_JUMP: Gesture<'static> = Gesture("MIX + B1-6", "PART");
const PART_JUMP_HINT: Hint = &[PART_JUMP];
const PART_JUMP_LEARN: Hint = &[PART_JUMP, Gesture("MIX + SEQ", "LEARN")];

/// A mixer cell's hint: `own` where it has words of its own, else the Part
/// jump, with learn's short form after it where `own` was the generic learn.
pub fn mixer_hint(own: Hint) -> Hint {
    if own == LEARN_HINT {
        PART_JUMP_LEARN
    } else if own.is_empty() {
        PART_JUMP_HINT
    } else {
        own
    }
}

const REV_HINTS: [Hint; 6] = [
    &[Gesture("REVERB ▸", ReverbType::NAMES[0])],
    &[Gesture("REVERB ▸", ReverbType::NAMES[1])],
    &[Gesture("REVERB ▸", ReverbType::NAMES[2])],
    &[Gesture("REVERB ▸", ReverbType::NAMES[3])],
    &[Gesture("REVERB ▸", ReverbType::NAMES[4])],
    &[Gesture("REVERB ▸", ReverbType::NAMES[5])],
];

const CHORUS_HINTS: [Hint; 4] = [
    &[Gesture("CHORUS ▸", MODE_NAMES[0])],
    &[Gesture("CHORUS ▸", MODE_NAMES[1])],
    &[Gesture("CHORUS ▸", MODE_NAMES[2])],
    &[Gesture("CHORUS ▸", MODE_NAMES[3])],
];

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

/// `addr`'s format under `ctx`: MODE names its kind's own modes.
fn fmt_of(addr: ParamAddr, fmt: ValFmt, ctx: &SlotCtx) -> ValFmt {
    match (addr.block, addr.param) {
        (BlockRef::Filter, FilterParams::MODE) => ValFmt::Names(ctx.kind.mode_labels()),
        _ => fmt,
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
        SlotBinding::Legacy(fmt) => View::Legacy {
            label: slot.label(),
            fmt: *fmt,
        },
        SlotBinding::Param(addr) if reads_type_b(addr, ctx) => View::Text {
            label: slot.label(),
            text: "TYPE B",
        },
        SlotBinding::Param(addr) => param(addr, slot.label(), fmt_of(addr, slot.format(), ctx)),
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
            Some(&PanelKnob { target, label }) => match target {
                PanelTarget::Filter(id) => {
                    let addr = ParamAddr::new(BlockRef::Filter, id);
                    let fmt = addr.spec().map_or(ValFmt::Uni, |s| s.fmt);
                    param(addr, label, fmt_of(addr, fmt, ctx))
                }
                PanelTarget::Route(_) | PanelTarget::Keybd | PanelTarget::JackAmount(_) => {
                    match target.route() {
                        Some(route) => View::Route { route, label },
                        None => View::Empty,
                    }
                }
            },
        },
        // OFF has no pots: dashes.
        SlotBinding::FxPanel(k) => match fx_panel::cells(ctx.fx).and_then(|c| c.get(k as usize)) {
            Some(c) => View::Param {
                addr: ParamAddr::new(BlockRef::VoiceFx, c.id),
                label: c.label,
                fmt: c.fmt,
                short: Some(c.short),
            },
            None => View::Empty,
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
        SlotBinding::OpPanel(op, k) => {
            match op_page::page_cells(ctx.op_kinds[op.index()]).get(usize::from(k)) {
                Some(&Some((id, label))) => {
                    let addr = ParamAddr::new(BlockRef::AlgoOp(op), id);
                    addr.spec()
                        .map_or(View::Empty, |s| param(addr, label, s.fmt))
                }
                _ => View::Empty,
            }
        }
        SlotBinding::DelayPanel(k) => {
            match crate::dsp::delay::char_cells(ctx.delay).get(usize::from(k)) {
                Some(&Some(id)) => {
                    let addr = ParamAddr::new(BlockRef::Delay, id);
                    addr.spec()
                        .map_or(View::Empty, |s| param(addr, s.label, s.fmt))
                }
                _ => View::Empty,
            }
        }
        SlotBinding::Machine(at) => machine_view(at, ctx),
        SlotBinding::Source => View::Source(ctx.src),
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

/// Whether slot `i` of `def` is a reverb panel cell: its text and step
/// are the TYPE's (`reverb_panel`).
pub fn is_reverb_panel(def: &BlockDef, i: usize) -> bool {
    matches!(
        def.params.get(i).map(|s| s.binding),
        Some(SlotBinding::ReverbPanel(..))
    )
}

/// SPD: a type-B slot's SPEED and HOLD read TYPE B, not a value (spec §
/// UI): no format, on the page or the host.
fn reads_type_b(addr: ParamAddr, ctx: &SlotCtx) -> bool {
    match addr.block {
        BlockRef::Env(s) => {
            matches!(addr.param, EnvParams::SPEED | EnvParams::HOLD_POS)
                && matches!(ctx.envs[s.index()], EnvKind::B(_))
        }
        _ => false,
    }
}

/// The format `addr` reads in under `ctx`: its modulator panel's cell for
/// an ENV or LFO param on that kind's page, else its spec's (a filter MODE
/// by KIND). One source for the pages and the host's `factory-sounds fmt`.
pub fn shown_fmt(addr: ParamAddr, ctx: &SlotCtx) -> Option<ValFmt> {
    if reads_type_b(addr, ctx) {
        return None;
    }
    let panel = match addr.block {
        BlockRef::Env(s) => Some(mod_panel::env_panel(ctx.envs[s.index()])),
        BlockRef::Lfo(s) => Some(mod_panel::lfo_panel(ctx.lfos[s.index()])),
        _ => None,
    };
    let cell = panel.and_then(|p| {
        p.slots.iter().flatten().find_map(|c| match *c {
            PanelSlot::Param { id, fmt, .. } if id == addr.param => Some(fmt),
            _ => None,
        })
    });
    cell.or_else(|| addr.spec().map(|s| fmt_of(addr, s.fmt, ctx)))
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
        // MIX mixes nothing with TYPE OFF.
        (BlockRef::VoiceFx, VoiceFxParams::MIX) => sound.params.fx.ty() == FxType::Off,
        // KIND lists built kinds only; with one it is fixed.
        (BlockRef::Filter, FilterParams::KIND) => FilterKind::BUILT.len() == 1,
        // A single-mode kind shows its mode fixed (spec § 7).
        (BlockRef::Filter, FilterParams::MODE) => sound.params.filter.kind().modes().len() == 1,
        // MORPH blends ALG A into ALG B: one algorithm, nothing to blend (#188).
        (BlockRef::Algo, AlgoParams::MORPH) => {
            let (a, b) = sound.params.algo.algs();
            a == b
        }
        // VOWEL moves formants and noise bands only.
        (BlockRef::Algo, AlgoParams::VOWEL) => !sound.params.algo.kinds().any_formant(),
        // What the operator's kind ignores (formant spec § 6).
        (BlockRef::AlgoOp(op), id) => !kinds::reads(sound.params.algo.kinds().get(op.index()), id),
        // What MODEL ignores (spec § 1 Dimming).
        (BlockRef::Modal, id) => !modal::reads(sound.params.modal.mode, id),
        // What the machine ignores (spec § 3).
        (BlockRef::TapeEngine, id) => !tape_engine::reads(sound.params.tape.model, id),
        // CLASSIC FREE plays the Part's clock: PHASE is RETRIG's start
        // alone (ADR 0177).
        (BlockRef::Lfo(s), crate::dsp::lfo::LfoParams::PHASE) => {
            let l = &sound.params.lfos[s.index()];
            l.lfo_type == crate::dsp::modulator::LfoType::Classic && l.sync == 0
        }
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
