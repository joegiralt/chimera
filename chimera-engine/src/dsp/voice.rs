use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use crate::addr::{BlockRef, Blocks, ParamAddr};
use crate::block::apply_offset;
use crate::dsp::algo::engine::{AlgoEngine, AlgoLive, SpanReach};
use crate::dsp::algo::kinds::OpKind;
use crate::dsp::algo::params::{AlgoOpParams, AlgoParams};
use crate::dsp::algo::plan::OPS;
use crate::dsp::dc_blocker::DcBlocker;
use crate::dsp::ease::{Ramp, at, step_of};
use crate::dsp::engines::{EngineCtx, EngineSlot, SlotKind};
use crate::dsp::envelope::{EnvMods, Envelope};
use crate::dsp::fast_exp2;
use crate::dsp::filter::stability::{self, STAB_COST};
use crate::dsp::filter::{
    FB_COST, FILTER_LEVEL_COST, Fed, Filter, FilterMods, FilterStability, Spread,
};
use crate::dsp::gate::AmpGate;
use crate::dsp::lpg::Lpg;
use crate::dsp::knobs::Gliding;
use crate::dsp::knobs::Knobs;
use crate::dsp::lfo::{Lfo, LfoClock};
use crate::dsp::modal::{ResonatorMode, SymPool};
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::dsp::voice_fx::VoiceFx;
use crate::dsp::voice_fx::params::{FxType, Place, VoiceFxParams};
use crate::hw::{Cost, MAX_VOICES, VOICE_CHAIN_BYTES, VOICE_RAM_BUDGET};
use crate::in_place::{by_value, uninit_at};
use crate::meter::Clip;
use crate::modulation::{
    FixedCol, MAX_MOD_SOURCES, ModRouting, ModSource, ModState, amount_scale, note_source,
};
use crate::params::{EngineType, EnvParams, FilterParams, OutParams, ParamSnapshot, Steal};
use crate::project::SourceId;
use crate::taps::{
    TAP_CAPTURE_COST, TAP_FOLD_COST, TAP_SHAPE_COST, TAP_VCA_COST, TapLane, TapOp, TapRoutes,
    TapRows, tap_vca,
};
use crate::voice_alloc::VoiceIdx;
use crate::{MidiNote, Velocity};

// ADR 0013: the voice pool fits D2 SRAM beside the DMA buffers, on both targets.
const _: () = assert!(core::mem::size_of::<[Voice; MAX_VOICES]>() <= VOICE_RAM_BUDGET);
// ADR 0051: a voice is its chain plus one engine, and the chain can't grow
// unnoticed.
const _: () = assert!(
    core::mem::size_of::<Voice>() <= VOICE_CHAIN_BYTES + core::mem::size_of::<EngineSlot>()
);

/// The voice's DC blocker corner (ADR 0060): after the last nonlinear
/// stage, −0.26 dB at 20 Hz, settled in 0.1 s.
pub const DC_HZ: f32 = 5.0;

/// An engine that ends with the voice past this (−80 dB) would end on a
/// step: the DC blocker's tail after a fast release (#487).
const END_STEP: f32 = 1.0e-4;

/// Complete voice signal chain:
/// [Engine] → [FX PRE] → [Filter] → [FX POST] → [tap-VCA] → [DC blocker] → [VCA]
/// Modulators: three envelopes, three LFOs
pub struct Voice {
    /// The engine it plays, and so the engine it last played.
    slot: EngineSlot,
    /// Its place in the pool: whom the sympathetic slots are lent to.
    id: VoiceIdx,
    sample_rate: u32,
    /// Slot rebuilds since `init_in_place`, wrapping: `reset` keeps it.
    rebuilds: u16,
    /// The FX slot, PRE or POST the filter (ADR 0155).
    fx: VoiceFx,
    filter: Filter,
    /// The filter's chip, this slot's (STABILITY, ADR 0168).
    stab: FilterStability,
    /// The FB amount the last block ended on: this block's ramps from it.
    fb_last: f32,
    /// The last block's LEVEL clip, for the LED: 0 while the clipper is
    /// off or the voice idle.
    filter_clip: Clip,
    /// The amp gate after the filter (ADR 0110).
    gate: AmpGate,
    /// DC from the stages before it (ADR 0060).
    dc: DcBlocker,
    /// The VCA's low-pass gate (ADR 0195): its state while LPG, DECAY or
    /// STRIKE is up.
    lpg: Lpg,
    /// OUT LEVEL × the engine's gain, ramped across each block.
    volume: Ramp,
    envs: [Envelope; 3],
    lfos: [Lfo; 3],
    /// The ENV destinations' sums from the last block's matrix (spec § Signal flow 1).
    env_mods: [EnvMods; 3],
    /// The sources' values from the last block; the VCA's other routes
    /// ramp from them.
    mod_values: [f32; MAX_MOD_SOURCES],
    /// The routes into VCA this voice plays.
    vca: VcaRoutes,
    /// The taps' amounts the last block played (`TapRoutes::amounts`), so
    /// every amount ramps from them: 8 B as stored `i8`s, scaled where read,
    /// not 32 as `f32`s (ADR 0101's 2,112 B voice chain).
    tap_amt: [[i8; 4]; 2],
    /// A note-on since the last block: its ENV destinations are recomputed
    /// with its own VEL and NOTE before the modulators run.
    retrigger: bool,
    active: bool,
    last_note: MidiNote,
    last_velocity: Velocity,
    /// The velocity the VCA's VEL played last block: a re-strike's ramps
    /// from it (#483).
    vca_velocity: Velocity,
    /// The source the last note, or the one waiting, plays (TAPE).
    last_source: Option<SourceId>,
    /// Samples left of a fade-out (`kill`, a switch); 0 when not
    /// fading. Never set on an inactive voice.
    fade: u16,
    /// What starts when the fade ends.
    after_fade: AfterFade,
    /// Key down since the last note-on.
    held: bool,
    /// The modulated settings of the last block rendered. A fade keeps
    /// them, so a new Sound never reaches the sound it fades out.
    played: ParamSnapshot,
    played_live: AlgoLive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterFade {
    Idle,
    /// The held note, on the Sound's new kind; a key up cancels it.
    Restart,
    /// A note-on that came mid-fade: `last_note`.
    Note,
}

/// The routes into VCA a voice plays, kept from the last block before a
/// fade (spec § 4; a fading voice keeps its routes).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct VcaRoutes {
    bits: u8,
    /// The stored amount per source, scaled where read: 9 B, not 33 as
    /// `f32`s, which keeps room in the voice chain (ADR 0101).
    amount: [i8; MAX_MOD_SOURCES],
}

impl VcaRoutes {
    /// The fixed VCA column's routes.
    fn of(m: &ModState) -> Self {
        let mut r = Self {
            bits: m.fixed_present(FixedCol::Vca),
            ..Self::default()
        };
        if r.bits != 0 {
            for (s, a) in r.amount.iter_mut().enumerate() {
                *a = m.fixed_amount(s, FixedCol::Vca);
            }
        }
        r
    }

    /// Source `s`'s `amount / 127`.
    fn scale(&self, s: usize) -> f32 {
        amount_scale(self.amount[s])
    }

    fn has(&self, source: usize) -> bool {
        self.bits & (1 << source) != 0
    }
}

// `reset` overwrites fields in place without dropping them.
const _: () = assert!(!core::mem::needs_drop::<Voice>());

/// Writes every field of `*p` but `slot`, `id`, `sample_rate` and `rebuilds`. The
/// pattern is exhaustive, so a new field fails to build until it is written
/// here.
macro_rules! write_chain {
    ($p:ident, { $($f:ident: $v:expr),* $(,)? }) => {{
        let _every_field = |v: &Voice| {
            let Voice { slot: _, id: _, sample_rate: _, rebuilds: _, $($f: _),* } = v;
        };
        $(addr_of_mut!((*$p).$f).write($v);)*
    }};
}

impl Default for Voice {
    fn default() -> Self {
        Self::new(chimera_hal::SAMPLE_RATE)
    }
}

impl Voice {
    /// The chain's floor: engine costs are bench per-voice minus this. The
    /// bench's `FLOOR` row (an Algo patch with every LEVEL at 0) measured 5 on
    /// 2026-09-27; 10 was kept, erring high. ADR 0060 adds the DC blocker
    /// (6.5 instructions a sample), the stages' ease (1.3) and the ramps of
    /// OUT LEVEL and filter LEVEL (about 2 each): 11.8 × 1.46 × 1.1 = 19,
    /// by ADR 0056's host method; 29, rounded up. The stages' ease is the
    /// Part's knobs' now (ADR 0112), a Part's not a voice's: kept, erring
    /// high. ADR 0110's amp gate costs nothing while the key is down; from
    /// note-off it scans the engine's block and the voice's (3 instructions
    /// a sample each) and ramps (4), a host count of 10 × 1.46 × 1.1 = 16 a
    /// sample in a release; billed always, 6 over the 30.
    pub const CHAIN_COST: Cost = Cost(36);

    /// A `kill` ramps to silence over this many samples (ADR 0027).
    pub const FADE: u16 = 2 * BLOCK_SIZE as u16;

    /// Cycles/sample of a voice playing `p` under `mods`.
    pub fn cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        EngineSlot::cost(p, mods)
            + Self::CHAIN_COST
            + Self::filter_cost(p, mods)
            + ModRouting::cost(p, mods)
            + Self::fx_cost(p, mods)
            + Self::tap_cost(p)
            + Self::gate_cost(p, mods)
    }

    /// The VCA's gate (ADR 0195): billed while LPG, DECAY or STRIKE is
    /// stored above 0 or routed.
    fn gate_cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        let routed = [OutParams::LPG, OutParams::DECAY, OutParams::STRIKE]
            .iter()
            .any(|&id| mods.moves_addr(ParamAddr::new(BlockRef::Out, id)));
        if p.out.gated() || routed {
            crate::dsp::lpg::LPG_COST
        } else {
            Cost::ZERO
        }
    }

    /// The filter as `p` and `mods` run it: the steady bill, the moving
    /// path once if a CUTOFF or RES route or (on ALGO) a FREQ or Q tap
    /// moves it, `TapShape::apply` once for those taps, and LEVEL's
    /// clipper while it can open (stored over ½, or routed). A clipper
    /// still running after LEVEL comes back is `held_level_extra`'s.
    pub fn filter_cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        let f = &p.filter;
        let kind = f.kind();
        let algo = p.engine() == EngineType::Algo;
        let shaped = algo && (p.taps.moving(FixedCol::Freq) || p.taps.moving(FixedCol::Q));
        let fb = Self::fb_runs(p, mods);
        let moving =
            shaped || fb || mods.fixed_moves(FixedCol::Freq) || mods.fixed_moves(FixedCol::Q);
        let when = |on: bool, c: Cost| if on { c } else { Cost::ZERO };
        f.filter_cost()
            + when(Self::stab_runs(p, mods), STAB_COST)
            + when(fb, FB_COST)
            + when(moving, kind.move_cost())
            + when(shaped, TAP_SHAPE_COST)
            + when(Self::level_may_open(p, mods), kind.level_cost())
    }

    /// What a Part's gliding knobs add to a voice playing `p` (ADR 0117): a
    /// glide steps the filter's and the FX slot's coefficients a few times a
    /// block, each step on the steady loop, over the steady bill. Nothing
    /// for a path a route or tap already bills moving. Admission counts it;
    /// shedding doesn't.
    pub fn glide_extra(p: &ParamSnapshot, mods: &ModState, g: Gliding) -> Cost {
        if g == Gliding::default() {
            return Cost::ZERO;
        }
        let algo = p.engine() == EngineType::Algo;
        let taps = |c| algo && p.taps.moving(c);
        let filter_moves = taps(FixedCol::Freq)
            || taps(FixedCol::Q)
            || Self::fb_runs(p, mods)
            || mods.fixed_moves(FixedCol::Freq)
            || mods.fixed_moves(FixedCol::Q);
        let when = |on: bool, c: Cost| if on { c } else { Cost::ZERO };
        let fx = g.fx && Self::fx_runs(p, mods) && !Self::fx_ramps(p, mods);
        when(g.filter && !filter_moves, p.filter.kind().glide_cost())
            + when(fx, p.fx.ty().glide_cost())
    }

    /// STABILITY spreads the voice: the 2040, LADDER or SEM, stored below 1
    /// or routed.
    fn stab_runs(p: &ParamSnapshot, mods: &ModState) -> bool {
        p.filter.kind().has_chip()
            && (p.filter.stability < 1.0
                || mods.moves_addr(ParamAddr::new(BlockRef::Filter, FilterParams::STAB)))
    }

    /// FB runs: stored off 0, or routed (ADR 0190). It runs the filter's
    /// moving path.
    fn fb_runs(p: &ParamSnapshot, mods: &ModState) -> bool {
        p.filter.fb != 0.0 || mods.moves_addr(ParamAddr::new(BlockRef::Filter, FilterParams::FB))
    }

    /// LEVEL's gate can open: stored over ½, or routed.
    fn level_may_open(p: &ParamSnapshot, mods: &ModState) -> bool {
        p.filter.level > 0.5
            || mods.moves_addr(ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL))
    }

    /// The taps the filter and the FX slot don't bill (on ALGO only): the
    /// tap-VCA once, and each wanted op's capture.
    fn tap_cost(p: &ParamSnapshot) -> Cost {
        if p.engine() != EngineType::Algo {
            return Cost::ZERO;
        }
        let vca = if p.taps.moving(FixedCol::Vca) {
            TAP_VCA_COST
        } else {
            Cost::ZERO
        };
        Self::live_taps(p)
            .wanted()
            .into_iter()
            .filter(|&w| w)
            .fold(vca, |c, _| c + TAP_CAPTURE_COST)
    }

    /// LEVEL's clipper still running (the ease landing on ½, then the
    /// fade) where `p` and `mods` no longer bill it: what the voice adds.
    pub fn held_level_extra(&self, p: &ParamSnapshot, mods: &ModState) -> Cost {
        if self.filter_level_running() && !Self::level_may_open(p, mods) {
            FILTER_LEVEL_COST
        } else {
            Cost::ZERO
        }
    }

    /// ALGO ops still running where the stored LEVELs no longer bill them
    /// (a LEVEL gliding to 0, ADR 0112): what the voice adds, until the
    /// glide lands.
    pub fn held_ops_extra(&self, p: &ParamSnapshot, mods: &ModState) -> Cost {
        let Some(run) = self.slot.algo_running() else {
            return Cost::ZERO;
        };
        let routed = mods.algo_levels_routed();
        let billed: [bool; OPS] = core::array::from_fn(|i| routed[i] || p.algo.ops[i].level > 0);
        if (0..OPS).all(|i| !run[i] || billed[i]) {
            return Cost::ZERO;
        }
        let live = core::array::from_fn(|i| routed[i] || run[i]);
        Cost(
            AlgoEngine::cost(&p.algo, &live)
                .0
                .saturating_sub(AlgoEngine::cost(&p.algo, &routed).0),
        )
    }

    /// A formant voice past the C8 grain clock the FRMT terms cover: the
    /// spans its reachable pitch adds (`HighNote`).
    pub fn high_note_extra(
        p: &ParamSnapshot,
        mods: &ModState,
        bend: u8,
        tune_cents: i8,
        note: MidiNote,
        slide: f32,
    ) -> Cost {
        HighNote::of(p, mods, bend, tune_cents).map_or(Cost::ZERO, |h| h.extra(note, slide))
    }

    /// An Algo voice's formant and noise operators' last `(fc, bw_eff)`.
    pub fn formant_view(&self) -> Option<[(f32, f32); crate::dsp::algo::plan::OPS]> {
        match &self.slot {
            EngineSlot::Algo(a) => Some(a.formant_view()),
            _ => None,
        }
    }

    /// A glide steal's ratio now on an Algo voice, 1 otherwise.
    pub fn algo_slide(&self) -> f32 {
        match &self.slot {
            EngineSlot::Algo(a) => a.slide(),
            _ => 1.0,
        }
    }

    /// What this voice's sounding note costs over `p`'s stored model
    /// (#183): Modal's MODE and MODES and TAPE's MODEL are fixed per note,
    /// so an edit under a sounding voice is billed at the costlier of the
    /// two; so is an ALGO operator's SKIRT (`held_skirt_extra`).
    pub fn held_model_extra(&self, p: &ParamSnapshot) -> Cost {
        if let EngineSlot::Algo(a) = &self.slot {
            return a.held_skirt_extra(&p.algo);
        }
        self.slot.playing_cost().map_or(Cost::ZERO, |held| {
            Cost(held.0.saturating_sub(self.slot.model_cost(p).0))
        })
    }

    /// The taps the voice plays: an FX tap acts only on FOLD; under any
    /// other type it is dimmed, unplayed and unbilled.
    fn live_taps(p: &ParamSnapshot) -> TapRoutes {
        let mut t = p.taps;
        if p.fx.ty() != FxType::Fold {
            for op in TapOp::ALL {
                t.clear(op, FixedCol::Fx);
            }
        }
        t
    }

    /// FOLD's tap moves P1 (on ALGO only).
    fn fx_tapped(p: &ParamSnapshot) -> bool {
        p.engine() == EngineType::Algo && p.fx.ty() == FxType::Fold && p.taps.moving(FixedCol::Fx)
    }

    /// Whether the FX slot runs: its type's `runs`, `moved` by a route of
    /// nonzero amount on P1 or FOLD's tap. The bill and the slot's gate
    /// read the same answer.
    pub fn fx_runs(p: &ParamSnapshot, mods: &ModState) -> bool {
        let moved = mods.fixed_moves(FixedCol::Fx) || Self::fx_tapped(p);
        p.fx.ty().runs(p.fx.pots[0], moved)
    }

    /// A route moves one of the slot's P1–P3 or MIX at audio rate.
    fn fx_routed(mods: &ModState) -> bool {
        use VoiceFxParams as X;
        [X::P1, X::P2, X::P3, X::MIX]
            .iter()
            .any(|&q| mods.moves_addr(ParamAddr::new(BlockRef::VoiceFx, q)))
    }

    /// The slot ramps every block: a route on its pots or MIX, or FOLD's tap.
    fn fx_ramps(p: &ParamSnapshot, mods: &ModState) -> bool {
        Self::fx_routed(mods) || Self::fx_tapped(p)
    }

    /// The FX slot as it runs: its type's cost, its ramp while it ramps,
    /// and FOLD's tapped loop.
    fn fx_cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        if !Self::fx_runs(p, mods) {
            return Cost::ZERO;
        }
        let t = p.fx.ty();
        let when = |on: bool, c: Cost| if on { c } else { Cost::ZERO };
        t.cost()
            + when(Self::fx_ramps(p, mods), t.ramp_cost())
            + when(Self::fx_tapped(p), TAP_FOLD_COST)
    }

    /// The sample rate is stored once (spec §3), not passed per call. A
    /// lone voice is voice 0.
    pub fn new(sample_rate: u32) -> Self {
        // SAFETY: `init_in_place` writes every field of the slot.
        unsafe { by_value(|slot| Self::init_in_place(slot, sample_rate, VoiceIdx::ALL[0])) }
    }

    pub fn init_in_place(
        slot: &mut MaybeUninit<Self>,
        sample_rate: u32,
        id: VoiceIdx,
    ) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; the slot is built in place
        // and every other (small) field is written once before
        // `assume_init_mut`.
        unsafe {
            EngineSlot::init_in_place(uninit_at(addr_of_mut!((*p).slot)), SlotKind::Algo);
            addr_of_mut!((*p).id).write(id);
            addr_of_mut!((*p).sample_rate).write(sample_rate);
            addr_of_mut!((*p).rebuilds).write(0);
            Self::init_chain(p);
            let v = slot.assume_init_mut();
            // Its own noise from boot, not the lone slot's voice 0.
            if let EngineSlot::Algo(a) = &mut v.slot {
                a.seed(id);
            }
            v
        }
    }

    /// Every field but `slot`, `id`, `sample_rate` and `rebuilds`, as `new` builds
    /// it: the one list that `init_in_place` and `reset` share.
    ///
    /// # Safety
    /// `p` must be valid for writes, aligned and unaliased, with
    /// `sample_rate` written.
    unsafe fn init_chain(p: *mut Self) {
        // SAFETY: the caller's guarantee; `sample_rate` is read, every
        // field is written by value, and none has drop glue (asserted
        // above) to skip.
        unsafe {
            let played = ParamSnapshot::default();
            write_chain!(p, {
                fx: VoiceFx::new(),
                filter: Filter::new(),
                stab: FilterStability::NEW,
                fb_last: 0.0,
                filter_clip: Clip::ZERO,
                gate: AmpGate::OPEN,
                dc: DcBlocker::new(DC_HZ, (*p).sample_rate),
                lpg: Lpg::NEW,
                volume: Ramp::default(),
                envs: [Envelope::new(); 3],
                lfos: [Lfo::new(); 3],
                env_mods: [EnvMods::NONE; 3],
                mod_values: [0.0; MAX_MOD_SOURCES],
                vca: VcaRoutes::default(),
                tap_amt: [[0; 4]; 2],
                retrigger: false,
                active: false,
                last_note: MidiNote::A4,
                last_velocity: Velocity::DEFAULT,
                vca_velocity: Velocity::DEFAULT,
                last_source: None,
                fade: 0,
                after_fade: AfterFade::Idle,
                held: false,
                played_live: AlgoLive::from_params(&played.algo),
                played: played,
            });
        }
    }

    /// Slot rebuilds since the voice was built, wrapping: for the tests
    /// and the bench.
    pub fn rebuilds(&self) -> u16 {
        self.rebuilds
    }

    /// The one path to `slot.rebuild`, counted.
    fn rebuild(&mut self, kind: SlotKind, pool: &mut SymPool) {
        self.rebuilds = self.rebuilds.wrapping_add(1);
        self.slot.rebuild(kind, pool.alloc_mut(), self.id)
    }

    /// Fading toward a restart of its held note on a changed Sound
    /// (`AfterFade::Restart`).
    pub fn restarts(&self) -> bool {
        self.after_fade == AfterFade::Restart
    }

    /// Its slot rings a pool set (Sympathetic with a halo).
    pub fn rings(&self) -> bool {
        self.slot.rings()
    }

    /// The filter settings the last block played, routes applied: for the
    /// tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn played_filter(&self) -> crate::params::FilterParams {
        self.played.filter
    }

    /// `addr`'s value the last block handed the engine, knobs and routes
    /// applied (stored units): for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn played_value(&self, addr: ParamAddr) -> f32 {
        self.played_live.get(addr).unwrap_or_else(|| {
            crate::addr::BlockRead::block(&self.played, addr.block)
                .map_or(f32::NAN, |b| b.get(addr.param))
        })
    }

    /// The MORPH the last block played, routes applied (stored units): for
    /// the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn played_morph(&self) -> f32 {
        self.played_live.morph
    }

    /// A glide steal's ratio now, 1 at rest: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn slide(&self) -> f32 {
        self.slot.slide()
    }

    /// The kind its slot holds.
    pub fn kind(&self) -> SlotKind {
        self.slot.kind()
    }

    /// Its engine slot: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn slot(&self) -> &EngineSlot {
        &self.slot
    }

    /// LFO `s`'s phase: for the tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn lfo_phase(&self, s: LfoSlot) -> f32 {
        self.lfos[s.index()].phase()
    }

    /// The sounding (or last) note's velocity.
    pub fn velocity(&self) -> Velocity {
        self.last_velocity
    }

    /// Idle, and no note waits for it: a Sympathetic slot rests and gives
    /// its lease back, and any promise it held is cancelled. For an engine
    /// that went quiet on its own; every other way to idle rebuilds.
    pub fn rest(&mut self, pool: &mut SymPool) {
        debug_assert!(!self.active);
        let kind = self.slot.kind();
        if kind != kind.resting() {
            self.rebuild(kind.resting(), pool);
        }
        pool.alloc_mut().cancel(self.id);
    }

    /// `params` plays another engine or Modal model than the slot holds.
    fn switched(&self, params: &ParamSnapshot) -> bool {
        SlotKind::of(params) != self.slot.kind()
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// A note on `params` needs its slot rebuilt: another engine or model,
    /// or a bare Sympathetic slot whose new note was promised a set.
    fn stale(&self, params: &ParamSnapshot, pool: &SymPool) -> bool {
        self.switched(params) || (self.slot.is_bare() && pool.alloc().promised(self.id))
    }

    /// A note-on on `params` starts now: the voice isn't fading, nor
    /// sounding a slot the note must rebuild (`note_on` then waits the
    /// fade).
    pub fn starts_now(&self, params: &ParamSnapshot, pool: &SymPool) -> bool {
        self.fade == 0 && !(self.active && self.stale(params, pool))
    }

    /// The bytes a Sympathetic note-on of `note` on `params` here clears:
    /// pure, for the `Instrument`'s per-block clear budget (spec § 4.8).
    /// One that starts now on its sounding engine clears nothing: a
    /// re-strike adds to it, and a glide steal writes only its rings'
    /// growth.
    pub fn sym_note_on_clear(
        &self,
        pool: &SymPool,
        note: MidiNote,
        params: &ParamSnapshot,
    ) -> usize {
        let adds = self.starts_now(params, pool) && params.pitch.steal == Steal::Glide;
        self.slot
            .sym_note_on_clear(pool, self.id, (note, adds, self.sample_rate))
    }

    /// On a fading voice, or one sounding another engine or model, the note
    /// waits for the fade-out and then starts clean. Returns whether it replaced
    /// a note still waiting, unheard. A TAPE note plays `source`.
    pub fn note_on(
        &mut self,
        note: MidiNote,
        velocity: Velocity,
        params: &ParamSnapshot,
        source: Option<SourceId>,
        ctx: &mut EngineCtx<'_>,
    ) -> bool {
        let replaced = self.after_fade == AfterFade::Note;
        self.held = true;
        if self.fade > 0 || (self.active && self.stale(params, ctx.sym)) {
            self.last_note = note;
            self.last_velocity = velocity;
            self.last_source = source;
            self.after_fade = AfterFade::Note;
            self.fade_out();
            return replaced;
        }
        self.trigger(note, velocity, params, source, ctx);
        replaced
    }

    fn trigger(
        &mut self,
        note: MidiNote,
        velocity: Velocity,
        params: &ParamSnapshot,
        source: Option<SourceId>,
        ctx: &mut EngineCtx<'_>,
    ) {
        let pool = &mut *ctx.sym;
        // Another engine or model starts clean, in place; so does a bare
        // Sympathetic slot whose new note has a set. A Sympathetic note
        // rings the set promised to its voice (the `Instrument` places each
        // as it starts), or plays bare.
        let kind = SlotKind::of(params);
        if self.stale(params, pool) {
            self.rebuild(kind, pool);
        }
        if kind != SlotKind::Modal(ResonatorMode::Sympathetic) {
            // A promise made for a note since replaced doesn't outlive it.
            pool.alloc_mut().cancel(self.id);
        }
        // The gate's strike (ADR 0195); a fresh note's gate starts afresh.
        self.lpg.strike(!self.active);
        if !self.active {
            // A fresh note: no ramp from the last note's settings, and no
            // ring left in the filter.
            self.filter.reset();
            self.stab.hold();
            self.fb_last = params.filter.fb;
            self.fx.hold();
            self.dc.reset();
            self.gate = AmpGate::OPEN;
            self.volume.hold();
            self.slot.hold();
            // An idle voice's ENV slots and last source values start as a
            // fresh voice's. Idle is silent: a release its VCA routes still
            // hold keeps the voice active (`vca_holds`).
            self.envs = [Envelope::new(); 3];
            self.mod_values = [0.0; MAX_MOD_SOURCES];
            self.tap_amt = [[0; 4]; 2];
            self.vca_velocity = velocity;
        }
        self.retrigger = true;
        self.gate.restart();
        self.stab
            .note_on(ctx.part, self.id.index() as u32, note.get());
        self.last_note = note;
        self.last_velocity = velocity;
        self.last_source = source;
        #[cfg(feature = "probe")]
        let t0 = crate::probe::now();
        self.slot
            .note_on(note, velocity, params, self.sample_rate, source, ctx);
        #[cfg(feature = "probe")]
        let t1 = crate::probe::now();
        #[cfg(feature = "probe")]
        crate::probe::acc(4, t1.wrapping_sub(t0));
        for e in &mut self.envs {
            e.note_on();
        }
        for (l, p) in self.lfos.iter_mut().zip(&params.lfos) {
            l.note_on(p);
        }
        self.active = true;
        // Until the first block renders, a fade has these to keep.
        self.played.clone_from(params);
        self.played_live = AlgoLive::from_params(&params.algo);
        #[cfg(feature = "probe")]
        crate::probe::acc(7, crate::probe::now().wrapping_sub(t1));
    }

    /// The note moves to `note` without a strike, gliding there on the
    /// voice's pitch path (ADR 0042, as a glide steal does, ADR 0065): a
    /// one-pole in log pitch, `glide_samples` 95 % of the way (the time
    /// constant a third of it). The envelopes and the modulators run on;
    /// NOTE reads the new note. A fading voice moves only the note that
    /// starts after its fade; an idle one has nothing to move. Its cost:
    /// three transcendentals (the start ratio, `Glide::new`), and on Modal
    /// the rings' growth (`retune_clear`), which the caller bills.
    pub fn retune(&mut self, note: MidiNote, glide_samples: u32, ctx: &mut EngineCtx<'_>) {
        if self.fade > 0 {
            if self.after_fade != AfterFade::Idle {
                self.last_note = note;
            }
            return;
        }
        if !self.active {
            return;
        }
        self.last_note = note;
        self.stab.retune(note.get());
        let tau = glide_samples as f32 / (3.0 * self.sample_rate as f32);
        self.slot.retune(note, (self.sample_rate, tau), ctx);
    }

    /// The bytes `retune` to `note` writes to the lines (`ModalEngine::fit`'s
    /// growth), for the `Instrument`'s clear budget: none fading or idle.
    pub fn retune_clear(&self, pool: &SymPool, note: MidiNote) -> usize {
        if self.fade > 0 || !self.active {
            return 0;
        }
        self.slot.retune_clear(pool, note, self.sample_rate)
    }

    /// What a voice playing `p` under `mods` costs more while it may
    /// retune (`EngineSlot::glide_cost`).
    pub fn glide_cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        EngineSlot::glide_cost(p, mods)
    }

    pub fn note_off(&mut self, ctx: &mut EngineCtx<'_>) {
        // ONESHOT ignores note-off (CHOMPI's sustain off): the gate stays
        // and the note ends at its END.
        if self.slot.sustains() {
            return;
        }
        self.held = false;
        if self.after_fade == AfterFade::Restart {
            self.after_fade = AfterFade::Idle;
        }
        self.slot.note_off(ctx);
    }

    /// Fade to silence over `FADE` samples, then go idle as a fresh voice.
    /// A second call mid-fade does not restart it. Returns whether a note
    /// that came mid-fade was dropped unheard.
    pub fn kill(&mut self) -> bool {
        let dropped = self.after_fade == AfterFade::Note;
        self.after_fade = AfterFade::Idle;
        self.fade_out();
        dropped
    }

    fn fade_out(&mut self) {
        if self.active && self.fade == 0 {
            self.fade = Self::FADE;
        }
    }

    /// The fade is over: back to fresh, then start a queued note.
    fn fade_ended(&mut self, params: &ParamSnapshot, ctx: &mut EngineCtx<'_>) {
        let (after, held) = (self.after_fade, self.held);
        let (note, velocity, source) = (self.last_note, self.last_velocity, self.last_source);
        let then_plays = after != AfterFade::Idle;
        self.reset(params, then_plays, ctx.sym);
        if then_plays {
            // The slot holds `params`' kind, clean: `trigger` has none to rebuild.
            self.held = true;
            self.trigger(note, velocity, params, source, ctx);
            if !held {
                self.note_off(ctx);
            }
        }
    }

    /// Back to the state `new` builds, in place: holding `params`' kind if
    /// a note follows, else its resting kind (no pool slot while idle).
    fn reset(&mut self, params: &ParamSnapshot, then_plays: bool, pool: &mut SymPool) {
        let kind = SlotKind::of(params);
        self.rebuild(if then_plays { kind } else { kind.resting() }, pool);
        // SAFETY: `self` is a valid, aligned, unaliased `Voice`.
        unsafe { Self::init_chain(self) }
    }

    /// The last block's LEVEL clip (`filter::clip_amount`).
    pub fn filter_clip(&self) -> Clip {
        self.filter_clip
    }

    /// The filter's LEVEL clipper runs next block (`LevelStage::running`):
    /// what its bill reads.
    pub fn filter_level_running(&self) -> bool {
        self.filter.level_running()
    }

    /// Sounding: its engine is active and, with routes into VCA, one of
    /// them holds it (spec § 4); a TAPE note with none, its key.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The strip's heads (`TapeEngine::heads`): none unless TAPE plays.
    pub fn heads(&self) -> crate::dsp::tape_engine::Heads {
        self.slot.heads()
    }

    /// Each LFO's phase, 0..1, for the LFO page's playhead (ADR 0177).
    pub fn lfo_view_phases(&self) -> [f32; 3] {
        self.lfos.each_ref().map(|l| l.view_phase())
    }

    /// The sources this voice reads or will, for the audio's `sounding`
    /// mask (spec § 4): only while active (an idle engine keeps its last
    /// source latched, and must not hold its row), the latched one, fading
    /// too, and the one a note queued behind the fade latched.
    pub fn reads(&self) -> [Option<SourceId>; 2] {
        if !self.active {
            return [None; 2];
        }
        let queued = self.after_fade != AfterFade::Idle;
        [self.slot.source(), self.last_source.filter(|_| queued)]
    }

    /// No route into VCA, and the engine can't end the note itself: a TAPE
    /// loop has no end (spec § 2), nor does VA, so its key holds it, as a
    /// VCA route from a source other than an ENV does.
    fn key_gates(&self) -> bool {
        self.vca.bits == 0 && matches!(self.slot, EngineSlot::Tape(_) | EngineSlot::Va(_))
    }

    /// A VCA source still holds the voice (spec § 4): an ENV slot per its
    /// TYPE and FORM, anything else while the key is held.
    fn vca_holds(&self) -> bool {
        let key = self.held;
        ModSource::ALL
            .iter()
            .filter(|s| self.vca.has(s.index()))
            .any(|s| match s.env_slot() {
                Some(e) => self.envs[e.index()].holds(key),
                None => key,
            })
    }

    /// `render_knobs` on the stored knobs, uneased: a voice on its own.
    pub fn render(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ParamSnapshot,
        mod_state: &ModState,
        ctx: &mut EngineCtx<'_>,
    ) {
        self.render_knobs(output, params, mod_state, &Knobs::UNPRIMED, None, ctx);
    }

    /// One block of `params` with its Part's `knobs` in place of the stored
    /// values they glide (ADR 0112), and of the FREQ/Q amounts. Its FREE
    /// LFOs play the Part's `clock` (ADR 0135); without one (a voice on its
    /// own) they run their own phase.
    pub fn render_knobs(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ParamSnapshot,
        mod_state: &ModState,
        knobs: &Knobs,
        clock: Option<&LfoClock>,
        ctx: &mut EngineCtx<'_>,
    ) {
        let sample_rate = self.sample_rate();

        // The Sound changed engine or model: fade the old one out; a held
        // note then restarts on the new one.
        if self.active && self.fade == 0 && self.switched(params) {
            if self.held {
                self.after_fade = AfterFade::Restart;
            }
            self.fade_out();
        }

        if !self.active {
            output.fill(0.0);
            self.filter_clip = Clip::ZERO;
            return;
        }

        // The FX slot runs, and fades its gate, as the allocator bills it: by
        // the stored settings. OFF runs nothing, so a switch to or from OFF
        // closes or opens the gate as the TYPE fade moves the side: the wet
        // share goes as t², both 20 ms, continuous at both ends. Kept so:
        // OFF's voice pays nothing once the fade ends, and no gate is left
        // open on a type that isn't there.
        let fx_on = Self::fx_runs(
            if self.fade == 0 { params } else { &self.played },
            mod_state,
        );
        // A fading voice keeps the settings it last played; else the stored
        // ones, with the Part's knobs on their way in their place. The
        // routes' offsets go on after the modulators.
        if self.fade == 0 {
            let (m, live) = (&mut self.played, &mut self.played_live);
            m.clone_from(params);
            *live = AlgoLive::from_params(&params.algo);
            knobs.apply(m, live);
        }
        // The modulators run every block, fading or not, from the settings
        // the voice plays (spec § Signal flow 1).
        let src = &self.played;
        let key = self.held;
        if self.fade == 0 {
            self.vca = VcaRoutes::of(mod_state);
        }
        let vca = self.vca;
        // The VCA's gain (spec § 4): 64 samples on the audio stack, zeroed
        // only with a route into VCA.
        let mut gain = (vca.bits != 0).then_some([0.0f32; BLOCK_SIZE]);
        if core::mem::take(&mut self.retrigger) {
            // The last block's ENV and LFO values; the new note's VEL and NOTE.
            let mut v = self.mod_values;
            v[ModSource::Vel.index()] = self.last_velocity.unit();
            v[ModSource::Note.index()] = note_source(self.last_note);
            let mut next = [EnvMods::NONE; 3];
            for d in 0..mod_state.num_dests() {
                if let BlockRef::Env(s) = mod_state.dest(d).block {
                    env_mod(&mut next[s.index()], mod_state, d, &v);
                }
            }
            self.env_mods = next;
        }
        let mut mod_values = [0.0f32; MAX_MOD_SOURCES];
        for (s, env) in EnvSlot::ALL.iter().zip(self.envs.iter_mut()) {
            let i = ModSource::of_env(*s).index();
            let feed = gain
                .as_mut()
                .filter(|_| vca.has(i))
                .map(|g| (g, vca.scale(i)));
            mod_values[i] = env.run_block(
                &src.envelopes[s.index()],
                &self.env_mods[s.index()],
                key,
                sample_rate,
                feed,
            );
        }
        for (s, lfo) in LfoSlot::ALL.iter().zip(self.lfos.iter_mut()) {
            let p = &src.lfos[s.index()];
            if let Some(c) = clock {
                lfo.follow(p, c.phases()[s.index()]);
            }
            mod_values[ModSource::of_lfo(*s).index()] = lfo.run_block(p, sample_rate);
        }
        mod_values[ModSource::Vel.index()] = self.last_velocity.unit();
        mod_values[ModSource::Note.index()] = note_source(self.last_note);
        // The other VCA sources ramp from their last block's value.
        for (s, &cur) in mod_values.iter().enumerate() {
            if let Some(gain) = gain.as_mut().filter(|_| vca.has(s))
                && ModSource::ALL[s].env_slot().is_none()
            {
                let a = vca.scale(s);
                let (from, step) = (
                    a * self.mod_values[s],
                    a * (cur - self.mod_values[s]) / BLOCK_SIZE as f32,
                );
                for (n, g) in gain.iter_mut().enumerate() {
                    *g += from + step * n as f32;
                }
            }
        }

        // A fading voice keeps the settings it last played.
        if self.fade == 0 {
            // Every routed destination gets its offset through its block's
            // spec (spec §4).
            let (m, live) = (&mut self.played, &mut self.played_live);
            live.routed = mod_state.algo_levels_routed();
            live.morph_routed =
                mod_state.moves_addr(ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH));
            // The fixed FREQ, Q and FX columns; VCA's is per sample, above.
            for c in [FixedCol::Freq, FixedCol::Q, FixedCol::Fx] {
                let off = knobs.fixed_sum(mod_state, c, &mod_values);
                let a = c.addr();
                if off != 0.0
                    && let Some(mut blk) = m.block_mut(a.block)
                {
                    apply_offset(&mut *blk, a.param, off);
                }
            }
            let mut next = [EnvMods::NONE; 3];
            for d in 0..mod_state.num_dests() {
                let a = mod_state.dest(d);
                if let BlockRef::Env(s) = a.block {
                    // ENV destinations reach their slot next block.
                    env_mod(&mut next[s.index()], mod_state, d, &mod_values);
                    continue;
                }
                let off = mod_state.sum_for(d, &mod_values);
                if off != 0.0 {
                    if live.offset(a, off) {
                        continue;
                    }
                    // Modulatable addresses are always Sound blocks (`voice_reads`).
                    if let Some(mut blk) = m.block_mut(a.block) {
                        apply_offset(&mut *blk, a.param, off);
                    }
                }
            }
            self.env_mods = next;
            // The wheel goes on after the routes: the same pitch path,
            // past PITCH's range.
            if knobs.bend() != 0.0 {
                m.pitch.pitch += knobs.bend();
            }
        }
        self.mod_values = mod_values;
        let (m, live) = (&self.played, &self.played_live);

        // The taps (spec § 6), live on ALGO only: each amount ramps from the
        // last block's, and the engine captures the ops some ramp reads.
        let (from, to) = if m.engine() == EngineType::Algo {
            let to = Self::live_taps(m).amounts();
            (core::mem::replace(&mut self.tap_amt, to), to)
        } else {
            self.tap_amt = [[0; 4]; 2];
            ([[0; 4]; 2], [[0; 4]; 2])
        };
        let mut rows = TapRows::new([0, 1].map(|o| from[o] != [0; 4] || to[o] != [0; 4]));

        // 1. Engine → raw oscillator output, and the tapped ops' rows.
        self.slot
            .render(output, m, live, sample_rate, ctx, &mut rows);
        // The amp gate (ADR 0110): ALGO and MODAL end by their own tail, so
        // a released note's filter may ring no louder than it; VCA routes
        // and TAPE's key gate the voice themselves.
        let gated = gain.is_none()
            && matches!(self.slot, EngineSlot::Algo(_) | EngineSlot::Modal(_))
            && !self.gate.idle(self.held);
        if gated && !self.held {
            self.gate.feed(output);
        }
        let lane = |c: FixedCol| {
            let (a, b) = (from.map(|o| o[c.index()]), to.map(|o| o[c.index()]));
            // A nonzero amount wants its op, so the rows are there.
            Some(TapLane {
                rows: rows.rows().filter(|_| a != [0; 2] || b != [0; 2])?,
                from: a.map(amount_scale),
                to: b.map(amount_scale),
            })
        };

        // 2. The FX slot before the filter, at the engine's reference. It
        // glides when no route moves its settings at audio rate: then only
        // a knob does, in steps (ADR 0117). Off, the kernels never read it
        // (the closing gate takes the per-sample path), so it isn't asked.
        let fx = self.fx.step(&m.fx, fx_on);
        let scale = m.engine().fx_scale();
        // FOLD's tap reaches it at either hook, wherever PLACE puts the fold.
        let tap = lane(FixedCol::Fx);
        let glide = !(fx_on && Self::fx_routed(mod_state));
        self.fx
            .apply(&fx, Place::Pre, output, scale, glide, tap.as_ref());

        // 3. Filter
        let mods = FilterMods {
            level_routed: mod_state
                .moves_addr(ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL)),
            freq: lane(FixedCol::Freq),
            q: lane(FixedCol::Q),
            // A route moves g and k at audio rate; without one, only a
            // knob's glide does.
            glide: !mod_state.filter_routed(),
            spread: if m.filter.kind().has_chip() {
                self.stab.step(stability::depth(m.filter.stability))
            } else {
                Spread::NONE
            },
            fb: Fed::new(
                core::mem::replace(&mut self.fb_last, m.filter.fb),
                m.filter.fb,
            ),
        };
        self.filter_clip = self.filter.process(output, &m.filter, sample_rate, &mods);

        // 4. The FX slot after it. FOLD's tap is live only under FOLD
        // (`live_taps`): leaving FOLD, its amount ramps out over the fade's
        // first block, as any tap amount moves.
        self.fx
            .apply(&fx, Place::Post, output, scale, glide, tap.as_ref());

        // 5. The tap-VCA, AM to ring, before the blocker: a tap correlated
        // with the signal leaves a DC, which the blocker takes.
        if let Some(l) = lane(FixedCol::Vca) {
            tap_vca(output, &l);
        }

        // 6. DC from the engine, the FX slot, the filter and the tap-VCA:
        // none reaches the VCA (ADR 0060).
        self.dc.run(output);

        if gated {
            self.gate.apply(output, self.held);
        }

        // 7. The VCA, after the blocker, with the engine's output gain,
        // ramped (never snap).
        let v = self.volume.step(m.out.volume * self.slot.out_gain());
        // The engine ended this block on a step: the VCA's own ramp takes
        // the voice to 0 over it, no work added. A ringing filter fades
        // below (#374).
        let v = if !self.slot.is_active()
            && !self.filter.rings()
            && libm::fabsf(output[BLOCK_SIZE - 1]) * v.1 > END_STEP
        {
            (v.0, 0.0)
        } else {
            v
        };
        let (volume, dv) = (v.1, step_of(v, BLOCK_SIZE));
        // The block's last routed gain, × AMP's VEL: whether the lifetime
        // check below ends the voice through a fade.
        let mut last_gain = 0.0;
        // LPG, DECAY or STRIKE up: the gate is the VCA (ADR 0195), through
        // bank 2's door; closed, the VCA plays as ever.
        let gate = if m.out.gated() {
            gate::through(|d| (d.run)(self, output, gain.as_ref(), (v.0, dv)))
        } else {
            None
        };
        if let Some(g) = gate {
            last_gain = g.last;
        } else {
            self.lpg.idle();
            let m = &self.played;
            match &gain {
                None => {
                    // No route: the engine decides (spec § 4). No wildcard, so a new
                    // engine can't inherit the pass-through (VA gates: #148).
                    match &self.slot {
                        // TAPE's and VA's key ends it (`key_gates`), through the fade.
                        EngineSlot::Algo(_)
                        | EngineSlot::Modal(_)
                        | EngineSlot::Tape(_)
                        | EngineSlot::Va(_) => {
                            // Its own envelopes shape the sound: today's expression, bit for bit.
                            if dv == 0.0 {
                                for sample in output.iter_mut() {
                                    *sample *= volume;
                                }
                            } else {
                                for (i, sample) in output.iter_mut().enumerate() {
                                    *sample *= at(v.0, dv, i);
                                }
                            }
                        }
                    }
                }
                Some(gain) => {
                    let amp = |v: Velocity| 1.0 - m.out.vca_vel + m.out.vca_vel * v.unit();
                    let (was, vel) = (amp(self.vca_velocity), amp(self.last_velocity));
                    self.vca_velocity = self.last_velocity;
                    let k = volume * vel;
                    #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
                    if was != vel {
                        // A re-strike or steal of a sounding voice at another
                        // velocity: AMP's VEL ramps with OUT LEVEL, one ramp,
                        // never a step (#483); a multiply a sample fewer than
                        // OUT LEVEL's ramp below, so the bill stands.
                        let r = (v.0 * was, k);
                        let (from, step) = (r.0, step_of(r, BLOCK_SIZE));
                        for (i, (sample, g)) in output.iter_mut().zip(gain).enumerate() {
                            *sample *= at(from, step, i) * g.max(0.0).min(1.0);
                        }
                    } else if dv == 0.0 {
                        for (sample, g) in output.iter_mut().zip(gain) {
                            *sample *= k * g.max(0.0).min(1.0);
                        }
                    } else {
                        for (i, (sample, g)) in output.iter_mut().zip(gain).enumerate() {
                            *sample *= at(v.0, dv, i) * vel * g.max(0.0).min(1.0);
                        }
                    }
                    #[allow(clippy::manual_clamp)]
                    let g = gain[BLOCK_SIZE - 1].max(0.0).min(1.0);
                    last_gain = g * vel;
                }
            }
        }

        // Check if done
        self.active = self.slot.is_active();
        if !self.active && self.filter.rings() {
            // The engine ended, but a self-oscillating filter rings on
            // (#374): fade it out over this block instead of cutting it,
            // inside a steal, kill or switch fade too, which ends with the
            // engine. The next note starts it from rest (`trigger`).
            for (i, sample) in output.iter_mut().enumerate() {
                *sample *= (BLOCK_SIZE - 1 - i) as f32 / BLOCK_SIZE as f32;
            }
        }

        if self.fade > 0 {
            for sample in output.iter_mut() {
                self.fade = self.fade.saturating_sub(1);
                *sample *= f32::from(self.fade) / f32::from(Self::FADE);
            }
            if self.fade == 0 || !self.active {
                self.fade_ended(params, ctx);
            }
        }

        // A TAPE or VA note with no route into VCA ends at key up, through
        // the fade; under the gate, when it shuts.
        if self.active && self.fade == 0 && self.key_gates() && gate.is_none() && !self.held {
            self.after_fade = AfterFade::Idle;
            self.fade = Self::FADE;
        }

        // Lifetime by the VCA's routes (spec § 4), once per block: a voice
        // no routed source holds ends, through the fade if it still sounds.
        let holds = match gate {
            Some(g) => g.holds,
            None => self.vca.bits == 0 || self.vca_holds(),
        };
        if self.active && self.fade == 0 && !holds {
            if last_gain != 0.0 {
                self.after_fade = AfterFade::Idle;
                self.fade = Self::FADE;
            } else {
                // Silent, but the engine may still sound: back to fresh.
                self.reset(params, false, ctx.sym);
            }
        }
    }
}

/// A formant Sound's reach past the C8 grain clock (`AlgoEngine::
/// high_note_extra`), less the note: a Part's, taken once a block (its
/// route scans); each voice's extra is then a `fast_exp2` and a span count
/// an operator (ADR 0131's fixed work).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HighNote {
    /// Semitones over the note from A4: TRNSP, the PITCH offset, TUNING
    /// (up only), the bend's range and every PITCH route at full swing.
    st: f32,
    reach: SpanReach,
}

impl HighNote {
    /// Recost's reach on a formant Part, once a block on its first voice,
    /// ADR 0056's count on the bench-algo ELF (2026-10-13): `of` 483
    /// instructions and 9 divides at six FRMTs, `pitch_reach`'s two scans
    /// and swings and both `algo_routed` scans, 16 dests at 109 at most,
    /// 8 sources at 23 twice: ~2,700 instructions, 9 divides, 4,445 cycles
    /// a block: 69.5, rounded up.
    pub const PART_COST: Cost = Cost(70);

    /// Each formant voice's `extra`: 206 instructions and 7 divides at six
    /// FRMTs, ~215 with recost's glue, 430 cycles a block: 6.7, rounded up.
    pub const VOICE_COST: Cost = Cost(7);

    /// `None`: no FRMT to bill.
    #[inline(never)]
    pub fn of(p: &ParamSnapshot, mods: &ModState, bend: u8, tune_cents: i8) -> Option<HighNote> {
        if p.engine() != EngineType::Algo || !p.algo.kinds().all().contains(&OpKind::Formant) {
            return None;
        }
        let st = f32::from(p.algo.transpose)
            + p.pitch.semitones()
            + f32::from(tune_cents.max(0)) / 100.0
            + f32::from(bend)
            + crate::dsp::engines::pitch_reach(mods) / 100.0
            - 69.0;
        let fine_top = mods.algo_routed(&[AlgoOpParams::FINE]);
        Some(HighNote {
            st,
            reach: SpanReach::of(&p.algo, &mods.algo_levels_routed(), &fine_top),
        })
    }

    /// A voice at `note`, gliding from `slide` (its ratio, 1 at rest).
    pub fn extra(&self, note: MidiNote, slide: f32) -> Cost {
        let st = f32::from(note.get()) + self.st;
        self.reach
            .extra(440.0 * fast_exp2(st / 12.0) * slide.max(1.0))
    }

    /// What a voice at `note` is billed for the reach: its `extra` and
    /// `VOICE_COST`, and on the Part's `first` voice `PART_COST`.
    pub fn bill(&self, note: MidiNote, slide: f32, first: bool) -> Cost {
        let part = if first { Self::PART_COST } else { Cost::ZERO };
        self.extra(note, slide) + Self::VOICE_COST + part
    }
}

/// ENV destination `d`'s sum under `values`, into its slot's `n`.
fn env_mod(n: &mut EnvMods, mod_state: &ModState, d: usize, values: &[f32; MAX_MOD_SOURCES]) {
    let sum = mod_state.sum_for(d, values);
    match mod_state.dest(d).param {
        EnvParams::LEVEL if mod_state.present(d) != 0 => {
            #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
            let peak = sum.max(0.0).min(1.0);
            n.level = Some(peak);
        }
        EnvParams::TIME => n.time = sum,
        EnvParams::RISE => n.slides.rise = sum,
        EnvParams::FALL => n.slides.fall = sum,
        EnvParams::SHAPE => n.slides.shape = sum,
        _ => {}
    }
}

pub mod gate;

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec § 3: a fresh note starts without a ramp, even on a voice
    /// whose engine went quiet on its own (inactive, never reset).
    #[test]
    fn a_note_after_silence_starts_without_a_ramp() {
        let p = ParamSnapshot::default();
        let mut pool = SymPool::boxed();
        let none = crate::source_table::GateView::empty();
        let ctx = &mut EngineCtx {
            sym: &mut pool,
            src: &none,
            part: crate::project::PartId::ALL[0],
            tune: 0.0,
        };
        let mut v = Voice::new(chimera_hal::SAMPLE_RATE);
        v.note_on(MidiNote::A4, Velocity::DEFAULT, &p, None, ctx);
        let mut b = [0.0f32; BLOCK_SIZE];
        v.render(&mut b, &p, &ModState::new(), ctx);
        assert!(v.filter.last_g().is_some());
        v.active = false; // its engine went quiet
        v.note_on(MidiNote::A4, Velocity::DEFAULT, &p, None, ctx);
        assert!(v.filter.last_g().is_none());
        // A retrigger of a sounding voice keeps it.
        v.render(&mut b, &p, &ModState::new(), ctx);
        v.note_on(MidiNote::A4, Velocity::DEFAULT, &p, None, ctx);
        assert!(v.filter.last_g().is_some());
    }
}
