//! A page slot as it reads now (filter-routing spec § UI): params, the
//! filter kind's panel and route views resolve here against the
//! Sound, so pages, vizzes and encoders all read by address.

use crate::addr::{BlockRead, BlockRef, Op, ParamAddr};
use crate::block::ValFmt;
use crate::dsp::algo::kinds::{self, OpKind};
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::algo::plan::OPS;
use crate::dsp::chorus::{ChorusMode, ChorusParams, MODE_NAMES};
use crate::dsp::delay::{DELAY_SPECS, DelayParams, DelaySync, DelayType};
use crate::dsp::drum::laws::spec;
use crate::dsp::drum::params::PadParams;
use crate::dsp::drum::sound::{PadFilter, PadSound, RouteId};
use crate::dsp::drum::types::{Kit, Knob, Machine, PadId, Tonal};
use crate::dsp::filter::FilterKind;
use crate::dsp::lfo::LfoParams;
use crate::dsp::modal::{self, ModalParams, ResonatorMode};
use crate::dsp::modulator::{
    EnvForm, EnvSlot, EnvSpeed, EnvType, Func, FuncMode, LfoForm, LfoSlot, LfoType, pick,
};
use crate::dsp::reverb::{ReverbParams, ReverbType, in_plate_zone};
use crate::dsp::tape_engine::machine;
use crate::dsp::tape_engine::{
    self, Choice, Direction, MachineIndex, TapeEngineParams as T, TapeMachine, Tron, wave,
};
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
use crate::storage::{SourceRef, Stem};
use crate::stream::units::SourceRate;
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
    /// TAPE's machine: its labels and cells.
    pub tape: TapeMachine,
    /// WALK once through (FORWARD or REVERSE), and TRON then: off, FADE
    /// ends in HOLD; on, LENGTH reads the tape's seconds. `None` around a
    /// loop, or off WALK.
    pub once: Option<Tron>,
    /// TAPE's PERIOD is 0: an unpitched source plays MONO as POLY.
    pub unpitched: bool,
    /// SCRUB's KEY is POSITION: FINGER's hint says a key places it.
    pub keys_place: bool,
    /// TAPE's SOURCE as SRC reads it.
    pub src: SourceState,
    /// The chorus MODE the CHORUS send hints.
    pub chorus: ChorusMode,
    /// The reverb panel REV and REV › MORE show (`reverb_panel::shown`).
    pub rev: ReverbType,
    /// The delay's TYPE: the other TYPE's cells dim (`delay_note`).
    pub delay: DelayType,
    /// The delay's SYNC: under it the TIME cell is the note (ADR 0302).
    pub delay_sync: DelaySync,
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
    /// Who takes each MIDI CONFIG › CONTROL channel that loses it, two bits
    /// each (`control::clashes`, ADR 0254): their cells dim.
    pub clash: u16,
    /// The selected pad's machine: which knob cells it shows. `None`
    /// where the `BlockRead` holds no pad (a Sound's params): every knob
    /// cell is then empty.
    pub pad: Option<Machine>,
    /// The Part's selected pad: what PAD shows and the header names.
    /// `SlotCtx::read` can't see it and takes pad 1; `UiState::ctx` sets
    /// it.
    pub sel_pad: PadId,
    /// The machine of the kit's TONAL pad; `None` with TONAL OFF, and
    /// where `SlotCtx::read` sees no kit.
    pub tonal: Option<Machine>,
    /// The selected pad's filter KIND: which panel its FLT cells show.
    pub pad_kind: FilterKind,
    /// The selected pad's route the route cells edit: the UI's
    /// (`UiState::ctx`), route 1 elsewhere.
    pub route: RouteId,
    /// A DRUM Part's pages: `kind` is the pad's, and FLT › MOD's SEM
    /// OUTPUT the pad's MODE.
    pub drum: bool,
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
        let tape = TapeMachine::from_index(MachineIndex(get(BlockRef::TapeEngine, T::MODEL) as u8));
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
            tape,
            once: (tape == TapeMachine::Walk
                && Direction::from_value(get(BlockRef::TapeEngine, T::MODE))
                    .is_some_and(Direction::once))
            .then(|| Tron::from_value(get(BlockRef::TapeEngine, T::TRON)).unwrap_or(Tron::Off)),
            unpitched: get(BlockRef::TapeEngine, T::PERIOD) == 0.0,
            keys_place: get(BlockRef::TapeEngine, T::KEY) == 0.0,
            src: SourceState::None,
            rev: reverb_panel::shown(rev.kind),
            // The project's, not a Sound's: `UiState::ctx` sets it.
            delay: DelayType::Tape,
            delay_sync: DelaySync::Free,
            plate: in_plate_zone(&rev),
            ring: RingIdx::ALL[0],
            clock: ClockSource::Internal,
            op_kinds: Op::ALL
                .map(|op| OpKind::clamped(get(BlockRef::AlgoOp(op), AlgoOpParams::WAVE) as u8)),
            pages: crate::ui::theme_settings::ThemeSettings::DEFAULT.pages,
            clash: 0,
            pad: params.block(BlockRef::Pad).and_then(|b| {
                Machine::ALL
                    .get(b.get(PadParams::MACHINE) as usize)
                    .copied()
            }),
            sel_pad: PadId::ALL[0],
            tonal: None,
            pad_kind: params
                .block(BlockRef::PadFilter)
                .map_or(PadSound::DEFAULT.filter().kind(), |b| {
                    FilterKind::from_index(b.get(FilterParams::KIND))
                }),
            route: RouteId::ALL[0],
            drum: false,
        }
    }

    /// This context on `kit` with pad `id` selected: its machine, and the
    /// TONAL pad's. Inlined into each caller, `UiState::ctx` and bank 2's
    /// `ed` (ADR 0285), so bank 1 holds no second copy.
    #[inline(always)]
    pub fn on_kit(self, kit: &Kit, id: PadId) -> Self {
        Self {
            pad: Some(kit.pad(id).machine()),
            sel_pad: id,
            tonal: match kit.tonal() {
                Tonal::Off => None,
                Tonal::Pad(p) => Some(kit.pad(p).machine()),
            },
            pad_kind: kit.pad(id).sound().filter().kind(),
            ..self
        }
    }

    /// This context on a DRUM Part: FLT shows the selected pad's KIND.
    /// Inlined as `on_kit` is.
    #[inline(always)]
    pub fn on_drum(self) -> Self {
        Self {
            drum: true,
            kind: self.pad_kind,
            ..self
        }
    }

    /// This context with the pad's route `route` selected.
    pub fn on_route(self, route: RouteId) -> Self {
        Self { route, ..self }
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
            pad: self.pad.map(|m| (self.sel_pad, m)),
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

    /// This context under the project delay `fx` (a Sound's params hold
    /// no FX): which TYPE's cells dim, and what TIME turns.
    pub fn on_delay(self, fx: &DelayParams) -> Self {
        Self {
            delay: fx.kind(),
            delay_sync: fx.sync(),
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

    /// This context under ORBIT's `clock`.
    pub fn on_clock(self, clock: ClockSource) -> Self {
        Self { clock, ..self }
    }
}

/// The engines' models a page's name follows: MDL2 MODAL's, the model
/// page TAPE's machine, a DRUM pad's page its pad and machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Models {
    pub modal: ResonatorMode,
    pub tape: TapeMachine,
    pub pad: Option<(PadId, Machine)>,
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
            tape: TapeMachine::Walk,
            pad: None,
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
    /// Streamed, and the card condemned its file whole (#609): it plays,
    /// dropping out over the records that won't de-frame.
    Damaged,
    /// TRANSWAVE on a source that isn't one.
    NotTranswave,
    /// TRON on a zone set (the streamed TRON's).
    ZonesOnTron,
    /// A streamed source the note would need in RAM (slice 1 § 2.3).
    NeedsRam(tape_engine::NeedsRam),
    Held {
        rate: SourceRate,
        frames: u32,
        cost: Cost,
        name: Option<SoundName>,
        /// Streamed: the notes at ROOT the card's budget fits
        /// (`admit::notes_at_root`), the band's polyphony.
        notes: Option<u8>,
    },
    /// The cell on a card row, not loaded: its head once read (`None`, its
    /// stem stands in), its cost, and the pool's free bytes.
    Card {
        stem: Stem,
        head: Option<(SoundName, u16, u32)>,
        cost: Cost,
        free: Cost,
    },
    /// A card read failed: until the next input.
    Failed(crate::ui::source_cell::SourceNote),
}

impl SourceState {
    /// What a note-on on `p` finds for `r` in `pool`.
    pub fn of(p: &T, r: Option<SourceRef>, pool: &Pool) -> Self {
        let Some(r) = r else {
            return SourceState::None;
        };
        let Some(id) = pool.find_source(r) else {
            return SourceState::Absent(Absence::Missing);
        };
        if let Some(why) = pool.absence(id) {
            return SourceState::Absent(why);
        }
        // Only TAPE streams: no scan, no words in a build without it.
        if cfg!(feature = "tape") && pool.streams().is_some_and(|reg| reg.damaged(id)) {
            return SourceState::Damaged;
        }
        let Some(rate) = SourceRate::new(r.rate) else {
            return SourceState::Absent(Absence::Missing);
        };
        let held = SourceState::Held {
            rate,
            frames: r.frames,
            cost: pool.source(id).map_or(Cost::default(), |e| e.cost()),
            name: None,
            notes: None,
        };
        match pool.source_view(r) {
            Some(v) if wave::playable(p.way(), &v) => held,
            Some(_) if p.machine == TapeMachine::Wave => SourceState::NotTranswave,
            None if pool.streamed(r) => match tape_engine::needs_ram(p) {
                Some(n) => SourceState::NeedsRam(n),
                None => held.streaming(p.tape > 0.0),
            },
            _ => SourceState::ZonesOnTron,
        }
    }

    /// A held source's name, once its head is known.
    pub fn named(self, n: Option<SoundName>) -> Self {
        match self {
            SourceState::Held {
                rate,
                frames,
                cost,
                notes,
                ..
            } => SourceState::Held {
                rate,
                frames,
                cost,
                name: n,
                notes,
            },
            s => s,
        }
    }

    /// A held source as streamed: its notes at ROOT, wow billed when TAPE
    /// runs (`tape_runs`).
    pub fn streaming(self, wow: bool) -> Self {
        match self {
            SourceState::Held {
                rate,
                frames,
                cost,
                name,
                ..
            } => {
                let n = crate::stream::admit::notes_at_root(rate, wow);
                SourceState::Held {
                    rate,
                    frames,
                    cost,
                    name,
                    // No more streams than voices.
                    notes: Some(n.min(crate::hw::MAX_VOICES as u32) as u8),
                }
            }
            s => s,
        }
    }

    /// A streamed source's notes re-counted with TAPE routed: its wow is
    /// billed whatever TAPE stores.
    pub fn tape_routed(self, routed: bool) -> Self {
        match self {
            SourceState::Held { notes: Some(_), .. } if routed => self.streaming(true),
            s => s,
        }
    }

    /// The focus band's: `CHOIR · 24K · 1.5 S · 41K`, a streamed one's
    /// `CHOIR · 24K · 7 STREAMS` (the notes at ROOT the card fits), or
    /// why it won't play.
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
                name,
                notes: Some(n),
                ..
            } => {
                // A name leaves no room for the length (32 B).
                match name {
                    Some(name) => {
                        let _ = write!(buf, "{} · {}K · ", name.as_str(), rate.get() / 1000);
                    }
                    None => {
                        let _ = write!(buf, "{}K · ", rate.get() / 1000);
                        seconds(buf, rate.get(), frames);
                        let _ = buf.write_str(" · ");
                    }
                }
                let word = if n == 1 { "STREAM" } else { "STREAMS" };
                let _ = write!(buf, "{n} {word}");
            }
            SourceState::Held {
                rate,
                frames,
                cost,
                name,
                notes: None,
            } => tail(buf, name, Some((rate.get(), frames)), cost),
            SourceState::Card {
                stem,
                head: None,
                cost,
                ..
            } => {
                let _ = write!(buf, "{:08X} · ", stem.get());
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
            SourceState::Absent(Absence::StreamsFull) => crate::ui::source_cell::STREAMS_FULL,
            SourceState::Absent(Absence::Fragmented) => crate::ui::source_cell::FRAGMENTED,
            SourceState::Absent(Absence::NeedsRam(r)) => crate::ui::source_cell::refusal_words(r),
            SourceState::Damaged => crate::ui::source_cell::FILE_DAMAGED,
            SourceState::NotTranswave => "NOT A TRANSWAVE",
            SourceState::ZonesOnTron => "ZONES: NOT TRON",
            SourceState::NeedsRam(n) => crate::ui::source_cell::needs_ram_words(n),
            SourceState::Failed(w) => w.words(),
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
            SourceState::Absent(Absence::StreamsFull) => buf.write_str("FULL"),
            SourceState::Absent(Absence::Fragmented) => buf.write_str("COPY"),
            SourceState::Absent(Absence::NeedsRam(r)) => {
                buf.write_str(crate::ui::source_cell::refusal_cell(r))
            }
            SourceState::Damaged => buf.write_str("BAD FILE"),
            SourceState::NotTranswave => buf.write_str("NO WAVE"),
            SourceState::ZonesOnTron => buf.write_str("ZONES"),
            SourceState::NeedsRam(_) => buf.write_str("RAM"),
            SourceState::Held { rate, frames, .. } => {
                seconds(buf, rate.get(), frames);
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
    /// DRUM's PAD: the Part's selected pad.
    SelectPad,
    /// DRUM's ROUTE: the selected pad's route.
    SelectRoute,
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
            View::SelectPad => "PAD",
            View::SelectRoute => "ROUTE",
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
            View::SelectPad => ValFmt::Names(&PAD_NUMBERS),
            View::SelectRoute => ROUTE_FMT,
            View::Legacy { fmt, .. } | View::Param { fmt, .. } => fmt,
            View::Route { route, .. } => route.fmt(),
            View::Orbit(c) => c.fmt(),
        }
    }

    /// The focus glyph: the param's spec's, else ARC; SOURCE none, PAD
    /// its machine's icon.
    pub fn glyph(&self) -> FocusGlyph {
        match self {
            View::Source(_) => return FocusGlyph::None,
            // The pad's machine, large.
            View::SelectPad => return FocusGlyph::Drum,
            _ => {}
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
        self.words(ctx).hint()
    }

    /// `hint`'s words by kind (ADR 0279): the fine turn and the mixer's
    /// PART jump join the plain, the mapped and the pad's.
    pub fn words(&self, ctx: &SlotCtx) -> Words {
        match self {
            View::Route { .. } => return Words::Own(crate::ui::mod_grid::DELETE),
            View::SelectPad | View::SelectRoute => return Words::Pad,
            _ => {}
        }
        match self.addr() {
            // Every cell of a pad's pages steps the pad.
            Some(a) if a.block.is_pad() => Words::Pad,
            Some(ParamAddr {
                block: BlockRef::Out,
                param,
            }) if param == OutParams::SEND3 => Words::Own(REV_HINTS[ctx.rev.index()]),
            Some(ParamAddr {
                block: BlockRef::Out,
                param,
            }) if param == OutParams::SEND1 => Words::Own(CHORUS_HINTS[ctx.chorus as usize]),
            Some(a) if matches!(a.block, BlockRef::OrbitRing(_)) && a.param == SceneRing::MUTE => {
                Words::Own(MUTE_HINT)
            }
            // MIX + turn snaps to an onset only on a held source
            // (`turn_window`); elsewhere it is the plain snap.
            Some(a)
                if a.block == BlockRef::TapeEngine
                    && matches!(a.param, T::START | T::END)
                    && matches!(ctx.src, SourceState::Held { .. }) =>
            {
                Words::Own(ONSET_HINT)
            }
            Some(a)
                if a.block == BlockRef::TapeEngine
                    && a.param == T::POSITION
                    && ctx.tape == TapeMachine::Scrub
                    && ctx.keys_place =>
            {
                Words::Own(KEY_HINT)
            }
            Some(a) if a.block == BlockRef::Random => Words::Own(ROLL_HINT),
            Some(a) if cfg!(feature = "midi-map") && Mappable::new(a).is_some() => Words::Learn,
            _ => Words::Plain,
        }
    }
}

/// PAD's numbers, as the band and the header write them.
pub static PAD_NUMBERS: [&str; PadId::ALL.len()] = [
    "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12",
];

/// ROUTE's numbers, 1–4.
pub const ROUTE_FMT: ValFmt = ValFmt::OneBased(RouteId::ALL.len() as u8 - 1);

/// MIX + MINUS / PLUS steps the pad on a pad's pages (ADR 0265).
pub const PAD_HINT: Hint = &[Gesture("MIX + -/+", "PAD")];

/// MIX + SEQ learns a CC on the cell (MIDI map spec § 4): every mappable
/// cell's hint, but where the cell's own gesture leaves no room on the line.
pub const LEARN: Gesture<'static> = Gesture("MIX + SEQ", "LEARNS A CC");
/// Every mappable cell's hint; PUSH ENCODERS' replaces it when on.
pub const LEARN_HINT: Hint = &[LEARN];

/// MIX + B*n* in the mixer keeps the page and opens Part *n*'s (ADR 0044).
const PART_JUMP: Gesture<'static> = Gesture("MIX + B1-6", "PART");
const PART_JUMP_HINT: Hint = &[PART_JUMP];
/// Learn's short form, where another gesture shares its line.
pub const LEARN_SHORT: Gesture<'static> = Gesture("MIX + SEQ", "LEARN");
const PART_JUMP_LEARN: Hint = &[PART_JUMP, LEARN_SHORT];

/// A mixer cell's hint: its own words where it has them, else the Part
/// jump, with learn's short form after it on a mapped cell.
pub fn mixer_hint(w: Words) -> Hint {
    match w {
        Words::Learn => PART_JUMP_LEARN,
        Words::Plain => PART_JUMP_HINT,
        w => w.hint(),
    }
}

/// A cell's own words by kind (ADR 0279): none, MIX + SEQ's learn, the
/// pad's step, or words of its own the band adds nothing beside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Words {
    Plain,
    Learn,
    Pad,
    Own(Hint),
}

impl Words {
    pub fn hint(self) -> Hint {
        match self {
            Words::Plain => hint::NONE,
            Words::Learn => LEARN_HINT,
            Words::Pad => PAD_HINT,
            Words::Own(h) => h,
        }
    }
}

/// The words of slot `v` on `def`: a Part cell on a pad's page (DRUM
/// FLT's FX TYPE and PLACE) steps the pad too (`edits_pad`).
pub fn cell_words(def: &BlockDef, v: &View, ctx: &SlotCtx) -> Words {
    match v.addr() {
        Some(a) if def.edits_pad() && !a.block.is_pad() => Words::Pad,
        _ => v.words(ctx),
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

const CHORUS_HINTS: [Hint; 6] = [
    &[Gesture("CHORUS ▸", MODE_NAMES[0])],
    &[Gesture("CHORUS ▸", MODE_NAMES[1])],
    &[Gesture("CHORUS ▸", MODE_NAMES[2])],
    &[Gesture("CHORUS ▸", MODE_NAMES[3])],
    &[Gesture("CHORUS ▸", MODE_NAMES[4])],
    &[Gesture("CHORUS ▸", MODE_NAMES[5])],
];

const MUTE_HINT: Hint = &[Gesture("MIX + B1-5", "MUTES THAT RING")];
/// RANDOMIZE's cells (ADR 0232): a turn rewrites the Part's Sound.
pub const ROLL_HINT: Hint = &[Gesture("TURN", "ROLLS THE SOUND")];
/// START and END: MIX + a turn snaps to the next onset its way (#698).
pub const ONSET_HINT: Hint = &[Gesture("MIX + TURN", "NEXT ONSET")];
/// SCRUB's FINGER at KEY POSITION (ADR 0205): a key places the finger,
/// which then moves from there.
pub const KEY_HINT: Hint = &[Gesture("KEYS", "PLACE THE FINGER")];

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

/// `addr`'s format under `ctx`: MODE names its kind's own modes; a TAPE
/// cell reads its engine law's unit, bank 2's (`walk::tape_fmt_in`; a
/// closed door, the spec's).
fn fmt_of(addr: ParamAddr, fmt: ValFmt, ctx: &SlotCtx) -> ValFmt {
    match (addr.block, addr.param) {
        (BlockRef::Filter, FilterParams::MODE) => ValFmt::Names(ctx.kind.mode_labels()),
        (BlockRef::PadFilter, FilterParams::MODE) => ValFmt::Names(ctx.pad_kind.mode_labels()),
        (BlockRef::TapeEngine, id) => crate::bank2::ui_door(|u| u.tape_fmt(id, ctx.tape, ctx.once))
            .flatten()
            .unwrap_or(fmt),
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
        SlotBinding::SelectPad => View::SelectPad,
        SlotBinding::SelectRoute => View::SelectRoute,
        SlotBinding::Legacy(fmt) => View::Legacy {
            label: slot.label(),
            fmt: *fmt,
        },
        SlotBinding::Param(addr) if reads_type_b(addr, ctx) => View::Text {
            label: slot.label(),
            text: "TYPE B",
        },
        // The delay's TIME under SYNC is its note (ADR 0302): the slot
        // keeps its label and place.
        SlotBinding::Param(addr) if ctx.delay_sync == DelaySync::Sync && addr == DELAY_TIME => {
            param(DELAY_NOTE, slot.label(), DELAY_SPECS[13].fmt)
        }
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
        SlotBinding::SelectedRoute(id) => param(
            ParamAddr::new(BlockRef::PadRoute(ctx.route), id),
            slot.label(),
            slot.format(),
        ),
        SlotBinding::PadFilterPanel(k) => match filter_panel::knob(ctx.pad_kind, k) {
            Some(&PanelKnob {
                target: PanelTarget::Filter(id),
                label,
            }) if PadFilter::IDS.contains(&id) => {
                filter_cell(ParamAddr::new(BlockRef::PadFilter, id), label, ctx)
            }
            _ => View::Empty,
        },
        SlotBinding::FilterPanel(k) => match filter_panel::knob(ctx.kind, k) {
            None => View::Empty,
            Some(&PanelKnob { target, label }) => match target {
                // FLT › MOD's SEM OUTPUT on a DRUM Part: the pad's MODE
                // (the Sound's is masked there).
                PanelTarget::Filter(id) => {
                    let block = match ctx.drum && PadFilter::IDS.contains(&id) {
                        true => BlockRef::PadFilter,
                        false => BlockRef::Filter,
                    };
                    filter_cell(ParamAddr::new(block, id), label, ctx)
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
        SlotBinding::Machine(at) => machine_view(at, ctx),
        SlotBinding::Knob(k) => knob_view(k, ctx),
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

/// A filter panel's cell on `addr`, the Sound's or the pad's, in the
/// panel's word.
fn filter_cell(addr: ParamAddr, label: &'static str, ctx: &SlotCtx) -> View {
    let fmt = addr.spec().map_or(ValFmt::Uni, |s| s.fmt);
    param(addr, label, fmt_of(addr, fmt, ctx))
}

/// `def` edits the selected pad with slot `focus` touched: a pad's page,
/// or a pad cell on a Part's page (FLT › MOD's SEM OUTPUT on DRUM). The
/// header names the pad there, and MIX + MINUS / PLUS steps it.
pub fn edits_pad(def: &BlockDef, focus: usize, ctx: &SlotCtx) -> bool {
    def.edits_pad()
        || view(def, focus, ctx)
            .addr()
            .is_some_and(|a| a.block.is_pad())
}

/// A TAPE cell on the Sound's machine, labelled its way: DECK's after
/// MODEL or the model page's after TAPE, AGE and WOW (ADR 0178). A
/// machine lists only what it reads, so a cell past its list is empty.
fn machine_view(at: MachineSlot, ctx: &SlotCtx) -> View {
    let k = machine::machine(ctx.tape);
    let cells = match at {
        MachineSlot::Deck(_) => k.deck,
        MachineSlot::Page(_) => k.cells,
    };
    let (MachineSlot::Deck(i) | MachineSlot::Page(i)) = at;
    let Some(cell) = cells.get(usize::from(i)) else {
        return View::Empty;
    };
    let (id, label) = (cell.id, cell.label);
    let addr = ParamAddr::new(BlockRef::TapeEngine, id);
    let Some(spec) = addr.spec() else {
        return View::Empty;
    };
    param(addr, label, fmt_of(addr, spec.fmt, ctx))
}

/// A DRUM knob on the selected pad's machine, labelled and read its way;
/// empty where the machine has no such knob.
fn knob_view(k: Knob, ctx: &SlotCtx) -> View {
    let Some(m) = ctx.pad else {
        return View::Empty;
    };
    match spec(m).knob(k) {
        Some(s) => View::Param {
            addr: ParamAddr::new(BlockRef::Pad, PadParams::knob(k)),
            label: s.label,
            fmt: ValFmt::Knob(m, k),
            short: Some(s.short),
        },
        None => View::Empty,
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

/// WOW shapes TAPE's wow: at rest (TAPE 0, unrouted) there is none.
pub const WOW_AT_REST: &str = "TAPE AT 0 · NO WOW TO SHAPE";
/// LOOP's and PING's FADE under a VCA route: AMP's release ends the note.
pub const FADE_ASIDE: &str = "AMP RELEASE ENDS THE NOTE";
/// ONE's FADE under a VCA route: only HOLD, its last step, is heard.
pub const FADE_HOLD_ONLY: &str = "BELOW HOLD AMP RELEASE ENDS IT";
/// WALK's XFADE once through with TRON off: only LOOP and TRON come
/// round to a seam.
pub const XFADE_LOOP_ONLY: &str = "ONLY LOOP AND TRON CROSSFADE";
/// WALK's XFADE on PING-PONG, TRON on or off: it turns, it has no seam.
pub const XFADE_NO_SEAM: &str = "PING-PONG TURNS · NO SEAM";
/// WALK's RETRIG on LOOP and PING-PONG.
pub const RETRIG_ONCE_ONLY: &str = "CUTS ONLY ONCE THROUGH";
/// WALK's RETRIG once through with TRON on: TRON's notes stack.
pub const RETRIG_TRON_OFF: &str = "CUTS ONLY WITH TRON OFF";
/// The delay's TAPE cells (DLY › CHAR) under TYPE GRAIN, and GRAIN's
/// (DLY › GRAIN) under TAPE (ADR 0202).
pub const DELAY_TAPE_ONLY: &str = "TAPE TYPE ONLY";
pub const DELAY_GRAIN_ONLY: &str = "GRAIN TYPE ONLY";
/// A streamed loop's XFADE past what its loop head holds (spec § 2.3).
pub const XFADE_CAPPED: &str = "STREAMED · XFADE CAPPED AT 100 MS";

/// Why a TAPE cell is dimmed though its machine reads it: WOW with TAPE
/// at rest, LOOP's or PING's FADE while a VCA route holds the voice (one
/// release a note: AMP's). ONE keeps FADE live, its HOLD step the gate.
pub fn tape_note(addr: ParamAddr, sound: &Sound) -> Option<&'static str> {
    // Bank 2's (`ui::walk`): closed, no TAPE cell says why.
    crate::bank2::ui_door(|u| u.tape_note(addr, sound)).flatten()
}

/// Why `v`'s cell is dimmed, if it is a CONTROL channel another takes
/// (ADR 0254): bank 2's (`control::clash_note`); closed, nothing.
pub fn clash_note(v: &View, ctx: &SlotCtx) -> Option<&'static str> {
    if ctx.clash == 0 {
        return None;
    }
    crate::ui::cc::clash_note(v.addr()?, ctx.clash)
}

/// SETTINGS › TUNING's TUNING cell.
pub const TUNING: ParamAddr = ParamAddr::new(BlockRef::Part, crate::part::PartParams::TUNING);

/// What the focus band says on SETTINGS › TUNING's TUNING or MAPPING for
/// Part `p` (ADR 0240, 0259): a file's description, that MTS follows MIDI
/// in, how MAPPING lays the keys. Bank 2's (`tuning::note_in`): closed,
/// nothing.
pub fn tuning_note(
    addr: ParamAddr,
    t: &crate::ui::tuning::Tuned,
    p: crate::project::PartId,
    part: &crate::preset::Part,
) -> Option<crate::project::Line> {
    if addr.block != BlockRef::Part {
        return None;
    }
    crate::bank2::ui_door(|u| u.tuning_note(t, addr.param, p, part)).flatten()
}

/// What TONAL's focus band says when its pad's machine has no PITCH:
/// every note plays the same hit (ADR 0270).
pub fn tonal_note(addr: ParamAddr, ctx: &SlotCtx) -> Option<crate::project::Line> {
    use core::fmt::Write;
    if addr != ParamAddr::new(BlockRef::Kit, crate::dsp::drum::params::KitParams::TONAL) {
        return None;
    }
    let m = ctx.tonal.filter(|m| !m.pitched())?;
    let name = spec(m).name;
    let mut l = crate::project::Line::new("NO PITCH ON ");
    let _ = l.write_str(name.rsplit(' ').next().unwrap_or(name));
    Some(l)
}

/// What CHOKE's focus band says: the kit's voice groups, `GROUPS 6/8`
/// (ADR 0280). Bank 2's (`kit_groups`, `Ask::Note`): closed, nothing.
pub fn groups_note(
    addr: ParamAddr,
    kit: &crate::dsp::drum::types::Kit,
) -> Option<crate::project::Line> {
    if addr != crate::ui::held::CHOKE {
        return None;
    }
    match crate::bank2::groups(crate::ui::kit_groups::Ask::Note(kit))? {
        crate::ui::kit_groups::Said::Line(l) => Some(l),
        _ => None,
    }
}

/// What MAX VOICES' and RESERVE's focus band says (ADR 0281): on RESERVE
/// the reserves booked across the Parts, `RESERVES 3/8` (bank 2's,
/// `part_voices`); with the door closed, on either, that the rule is off.
#[inline(never)]
pub fn voices_note(
    addr: ParamAddr,
    parts: &[crate::preset::Part; crate::hw::MAX_PARTS],
) -> Option<crate::project::Line> {
    let cell = crate::ui::held::VoiceCell::of(addr)?;
    let mut out = None;
    let open = match cell {
        crate::ui::held::VoiceCell::Reserve => {
            crate::bank2::voices(crate::ui::part_voices::Ask::Note(parts, &mut out))
        }
        crate::ui::held::VoiceCell::Max => crate::bank2::ui_door(|_| ()),
    };
    match open {
        Some(()) => out,
        None => Some(crate::project::Line::new(RULE_OFF)),
    }
}

/// CHOKE's note (`groups_note`), else MAX VOICES' or RESERVE's
/// (`voices_note`): one out-of-line call for the renderer.
#[inline(never)]
pub fn kit_or_voices_note(
    addr: ParamAddr,
    kit: &crate::dsp::drum::types::Kit,
    parts: &[crate::preset::Part; crate::hw::MAX_PARTS],
) -> Option<crate::project::Line> {
    groups_note(addr, kit).or_else(|| voices_note(addr, parts))
}

/// MAX VOICES' and RESERVE's band with bank 2 closed: nothing checks
/// them (ADR 0281).
pub const RULE_OFF: &str = "NO VOICE RULE \u{b7} NEEDS UPDATE";

/// What the focus band says on an operator's CRSE, FINE or DETUN: the
/// ratio they come to (#986). Bank 2's (`notes::ratio`): closed, nothing.
pub fn ratio_note(addr: ParamAddr, sound: &Sound) -> Option<crate::project::Line> {
    let BlockRef::AlgoOp(op) = addr.block else {
        return None;
    };
    let mut out = None;
    crate::bank2::ui_door(|u| {
        u.notes(crate::ui::notes::Ask::Ratio {
            algo: &sound.params.algo,
            op,
            param: addr.param,
            out: &mut out,
        })
    });
    out
}

/// What the focus band says in its hint's place about a TAPE cell: why
/// it is dimmed (`tape_note`), or a streamed LOOP's XFADE past the 100 ms
/// its loop head holds. Bank 2's (`walk::band_note_in`): closed, nothing.
pub fn band_note(addr: ParamAddr, sound: &Sound, src: SourceState) -> Option<&'static str> {
    crate::bank2::ui_door(|u| u.band_note(addr, sound, src)).flatten()
}

/// The matrix's hint on its cursor, `ui::notes`' over the grid's own
/// (ADR 0292): the VCA column's words, bank 2's.
pub fn matrix_hint(m: &crate::ui::mod_grid::MatrixState) -> crate::ui::hint::Hint {
    let mut out = None;
    crate::bank2::ui_door(|u| u.notes(crate::ui::notes::Ask::Matrix(m, &mut out)));
    out.unwrap_or_else(|| m.hint())
}

/// A route moves TAPE: its wow runs whatever TAPE stores (`tape_runs`).
pub fn tape_routed(sound: &Sound) -> bool {
    let a = ParamAddr::new(BlockRef::TapeEngine, T::TAPE);
    sound.mod_state.moves_addr(a)
}

/// A fixed or inapplicable slot draws dimmed, and its encoder is ignored
/// (spec § UI "Dimmed").
pub fn dimmed(addr: ParamAddr, sound: &Sound) -> bool {
    match (addr.block, addr.param) {
        // AMP's VEL and BASE (ADR 0292) under the Algo/Modal pass-through
        // (spec § 5). A pad's VEL drives its own amp, routed or not (ADR 0269).
        (BlockRef::Out, OutParams::VCA_VEL | OutParams::VCA) => {
            sound.mod_state.routes_into(VCA) == 0
        }
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
        // What the machine ignores (spec § 3), and what can't be heard:
        // WOW with TAPE at rest, WALK's FADE under an AMP route (`fade_aside`).
        (BlockRef::TapeEngine, id) => {
            !tape_engine::reads(sound.params.tape.machine(), id) || tape_note(addr, sound).is_some()
        }
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
        // A pad's KIND and MODE, by its own kind.
        View::Param {
            addr:
                ParamAddr {
                    block: BlockRef::PadFilter,
                    param: FilterParams::KIND,
                },
            ..
        } => FilterKind::BUILT.len() == 1,
        View::Param {
            addr:
                ParamAddr {
                    block: BlockRef::PadFilter,
                    param: FilterParams::MODE,
                },
            ..
        } => ctx.pad_kind.modes().len() == 1,
        View::Param { addr, .. } => dimmed(addr, sound) || delay_note(addr, ctx).is_some(),
        _ => false,
    }
}

/// The focus readout's word for `addr` at display value `v` (or the held
/// choice `held`) where its cell abbreviates it: full words
/// (`walk::readout_word_in`, bank 2's: closed, none).
pub fn readout_word(addr: ParamAddr, v: f32, held: Option<u8>) -> Option<&'static str> {
    crate::bank2::ui_door(|u| u.readout_word(addr, v, held)).flatten()
}

/// The delay's TIME cell: the knob's ms, or under SYNC its note (ADR
/// 0302).
pub const DELAY_TIME: ParamAddr = ParamAddr::new(BlockRef::Delay, DelayParams::TIME_MS);
pub const DELAY_NOTE: ParamAddr = ParamAddr::new(BlockRef::Delay, DelayParams::NOTE);
const _: () = assert!(DELAY_SPECS[13].id.0 == DelayParams::NOTE.0);

/// Why a delay cell is dimmed: the other TYPE's (ADR 0202), bank 2's
/// (`walk::delay_note_in`; closed, none).
pub fn delay_note(addr: ParamAddr, ctx: &SlotCtx) -> Option<&'static str> {
    if addr.block != BlockRef::Delay {
        return None;
    }
    crate::bank2::ui_door(|u| u.delay_note(addr.param, ctx.delay)).flatten()
}
