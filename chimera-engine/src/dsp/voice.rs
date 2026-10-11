use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::BLOCK_SIZE;

use crate::addr::{BlockRef, Blocks, ParamAddr};
use crate::block::{ParamId, apply_offset};
use crate::dsp::algo::engine::{AlgoEngine, AlgoLive};
use crate::dsp::algo::params::AlgoParams;
use crate::dsp::algo::plan::OPS;
use crate::dsp::dc_blocker::DcBlocker;
use crate::dsp::drive::Drive;
use crate::dsp::ease::{Ramp, at, step_of};
use crate::dsp::engines::{EngineCtx, EngineSlot, SlotKind};
use crate::dsp::envelope::{EnvMods, Envelope};
use crate::dsp::filter::{FILTER_LEVEL_COST, Filter, FilterMods};
use crate::dsp::gate::AmpGate;
use crate::dsp::knobs::Gliding;
use crate::dsp::knobs::Knobs;
use crate::dsp::lfo::Lfo;
use crate::dsp::modal::{ResonatorMode, SymPool};
use crate::dsp::modulator::{EnvSlot, LfoSlot};
use crate::dsp::wavefolder::Wavefolder;
use crate::hw::{Cost, MAX_VOICES, VOICE_CHAIN_BYTES, VOICE_RAM_BUDGET};
use crate::in_place::{by_value, uninit_at};
use crate::meter::Clip;
use crate::modulation::{
    FixedCol, MAX_MOD_SOURCES, ModRouting, ModSource, ModState, amount_scale, note_source,
};
use crate::params::{
    DriveParams, EngineType, EnvParams, FilterParams, FolderParams, ParamSnapshot, Steal,
};
use crate::project::SourceId;
use crate::taps::{
    TAP_CAPTURE_COST, TAP_FOLD_COST, TAP_SHAPE_COST, TAP_VCA_COST, TapLane, TapRows, tap_vca,
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

/// Complete voice signal chain:
/// [Engine] → [Drive] → [Filter] → [Wavefolder] → [tap-VCA] → [DC blocker] → [VCA]
/// Modulators: three envelopes, three LFOs
pub struct Voice {
    /// The engine it plays, and so the engine it last played.
    slot: EngineSlot,
    /// Its place in the pool: whom the sympathetic slots are lent to.
    id: VoiceIdx,
    sample_rate: u32,
    /// Slot rebuilds since `init_in_place`, wrapping: `reset` keeps it.
    rebuilds: u16,
    drive: Drive,
    filter: Filter,
    /// The last block's LEVEL clip, for the LED: 0 while the clipper is
    /// off or the voice idle.
    filter_clip: Clip,
    folder: Wavefolder,
    /// The amp gate after the filter (ADR 0110).
    gate: AmpGate,
    /// DC from the stages before it (ADR 0060).
    dc: DcBlocker,
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

    /// The wavefolder, which runs once FOLD is 0.001 or more: the bench's
    /// FOLD row less 1 OP (measured 2026-09-28, rev V at 480 MHz), 43, and
    /// the offset of silence's fold (ADR 0060), 1 instruction a sample, 1.6
    /// by ADR 0056's host method.
    pub const FOLD_COST: Cost = Cost(45);
    /// FOLD, SYM or MIX routed, or a FOLD tap: the fold ramps every block.
    /// Recounted on the bench ELF at 1879169d (2026-10-07): the moving loop
    /// 45 instructions a sample (silence's fold per sample, ADR 0060) +
    /// 3.48 of block, 80, against the steady 24 + 3.48, 45: 35 (was 29,
    /// from 42 against 24).
    pub const FOLD_RAMP_COST: Cost = Cost(35);
    /// The drive stage, which runs once DRIVE is 0.001 or more. Measured:
    /// bench-t13d's DRIVE LO row (541) less that run's 1 OP (488) = 53;
    /// bench-t13c's DRIVE row (540) less that run's 1 OP (483) = 57.
    /// Billed as the larger: 57.
    pub const DRIVE_COST: Cost = Cost(57);
    /// DRIVE, TONE or MIX routed: the drive ramps every block, 36
    /// instructions a sample against the steady 22; 14 × 1.46 × 1.1 = 22.5.
    /// Unrouted, only the UI moves them: brief, in the headroom (ADR 0061).
    pub const DRIVE_RAMP_COST: Cost = Cost(23);

    /// A knob glide's fold and drive blocks: four steady loops' prologues,
    /// a few instructions each, no divide: under 1 a sample, billed 1
    /// (ADR 0117).
    pub const FOLD_GLIDE_COST: Cost = Cost(1);
    pub const DRIVE_GLIDE_COST: Cost = Cost(1);

    /// Cycles/sample of a voice playing `p` under `mods`.
    pub fn cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        EngineSlot::cost(p, mods)
            + Self::CHAIN_COST
            + Self::filter_cost(p, mods)
            + ModRouting::cost(p, mods)
            + Self::stage_cost(p, mods)
            + Self::tap_cost(p)
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
        let moving = shaped || mods.fixed_moves(FixedCol::Freq) || mods.fixed_moves(FixedCol::Q);
        let when = |on: bool, c: Cost| if on { c } else { Cost::ZERO };
        f.filter_cost()
            + when(moving, kind.move_cost())
            + when(shaped, TAP_SHAPE_COST)
            + when(Self::level_may_open(p, mods), kind.level_cost())
    }

    /// What a Part's gliding knobs add to a voice playing `p` (ADR 0117): a
    /// glide steps the filter, fold and drive coefficients a few times a
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
            || mods.fixed_moves(FixedCol::Freq)
            || mods.fixed_moves(FixedCol::Q);
        let (fold, drive) = Self::stage_runs(p, mods);
        let routed = |block, ps: [ParamId; 3]| {
            ps.iter()
                .any(|&q| mods.moves_addr(ParamAddr::new(block, q)))
        };
        let fold_ramps = routed(
            BlockRef::Folder,
            [
                FolderParams::FOLD,
                FolderParams::SYMMETRY,
                FolderParams::MIX,
            ],
        ) || taps(FixedCol::Fold);
        let drive_ramps = routed(
            BlockRef::Drive,
            [DriveParams::DRIVE, DriveParams::TONE, DriveParams::MIX],
        );
        let when = |on: bool, c: Cost| if on { c } else { Cost::ZERO };
        when(g.filter && !filter_moves, p.filter.kind().glide_cost())
            + when(g.fold && fold && !fold_ramps, Self::FOLD_GLIDE_COST)
            + when(g.drive && drive && !drive_ramps, Self::DRIVE_GLIDE_COST)
    }

    /// LEVEL's gate can open: stored over ½, or routed.
    fn level_may_open(p: &ParamSnapshot, mods: &ModState) -> bool {
        p.filter.level > 0.5
            || mods.moves_addr(ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL))
    }

    /// The taps the filter and folder don't bill (on ALGO only): the
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
        p.taps
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

    /// What this voice's sounding note costs over `p`'s stored model
    /// (#183): Modal's MODE and MODES and TAPE's MODEL are fixed per note,
    /// so an edit under a sounding voice is billed at the costlier of the
    /// two.
    pub fn held_model_extra(&self, p: &ParamSnapshot) -> Cost {
        self.slot.playing_cost().map_or(Cost::ZERO, |held| {
            Cost(held.0.saturating_sub(self.slot.model_cost(p).0))
        })
    }

    /// Whether the folder and the drive stage run: once the stored amount
    /// is 0.001 or more, or a route or (on ALGO) a tap of nonzero amount
    /// may raise it. The bill and each stage's gate read the same answer.
    pub fn stage_runs(p: &ParamSnapshot, mods: &ModState) -> (bool, bool) {
        let routed = |block, param| mods.moves_addr(ParamAddr::new(block, param));
        let tapped = p.engine() == EngineType::Algo && p.taps.moving(FixedCol::Fold);
        (
            p.folder.fold >= 0.001 || routed(BlockRef::Folder, FolderParams::FOLD) || tapped,
            p.drive.drive >= 0.001 || routed(BlockRef::Drive, DriveParams::DRIVE),
        )
    }

    /// The folder and the drive stage as they run, each with its ramp while
    /// a route moves one of its settings.
    fn stage_cost(p: &ParamSnapshot, mods: &ModState) -> Cost {
        let (fold, drive) = Self::stage_runs(p, mods);
        let routed = |block, params: [ParamId; 3]| {
            params
                .iter()
                .any(|&param| mods.moves_addr(ParamAddr::new(block, param)))
        };
        let tapped = p.engine() == EngineType::Algo && p.taps.moving(FixedCol::Fold);
        let mut c = Cost::ZERO;
        if fold {
            c = c + Self::FOLD_COST;
            let fp = [
                FolderParams::FOLD,
                FolderParams::SYMMETRY,
                FolderParams::MIX,
            ];
            if routed(BlockRef::Folder, fp) || tapped {
                c = c + Self::FOLD_RAMP_COST;
            }
            if tapped {
                c = c + TAP_FOLD_COST;
            }
        }
        if drive {
            c = c + Self::DRIVE_COST;
            if routed(
                BlockRef::Drive,
                [DriveParams::DRIVE, DriveParams::TONE, DriveParams::MIX],
            ) {
                c = c + Self::DRIVE_RAMP_COST;
            }
        }
        c
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
            slot.assume_init_mut()
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
                drive: Drive::new(),
                filter: Filter::new(),
                filter_clip: Clip::ZERO,
                folder: Wavefolder::new(),
                gate: AmpGate::OPEN,
                dc: DcBlocker::new(DC_HZ, (*p).sample_rate),
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
        if !self.active {
            // A fresh note: no ramp from the last note's settings, and no
            // ring left in the filter.
            self.filter.reset();
            self.drive.hold();
            self.folder.hold();
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
        }
        self.retrigger = true;
        self.gate.restart();
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
    /// loop has no end (spec § 2), so its key holds it, as a VCA route from
    /// a source other than an ENV does.
    fn key_gates(&self) -> bool {
        self.vca.bits == 0 && matches!(self.slot, EngineSlot::Tape(_))
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
        self.render_knobs(output, params, mod_state, &Knobs::UNPRIMED, ctx);
    }

    /// One block of `params` with its Part's `knobs` in place of the stored
    /// values they glide (ADR 0112), and of the FREQ/Q amounts.
    pub fn render_knobs(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ParamSnapshot,
        mod_state: &ModState,
        knobs: &Knobs,
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

        // The stages run, and fade their gates, as the allocator bills them:
        // by the stored settings.
        let (fold_on, drive_on) = Self::stage_runs(
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
            mod_values[ModSource::of_lfo(*s).index()] =
                lfo.run_block(&src.lfos[s.index()], sample_rate);
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
            // The fixed FREQ, Q and FOLD columns; VCA's is per sample, above.
            for c in [FixedCol::Freq, FixedCol::Q, FixedCol::Fold] {
                let off = knobs.fixed_sum(mod_state, c, &mod_values);
                let a = c.addr();
                if off != 0.0
                    && let Some(blk) = m.block_mut(a.block)
                {
                    apply_offset(blk, a.param, off);
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
                    if let Some(blk) = m.block_mut(a.block) {
                        apply_offset(blk, a.param, off);
                    }
                }
            }
            self.env_mods = next;
        }
        self.mod_values = mod_values;
        let (m, live) = (&self.played, &self.played_live);

        // The taps (spec § 6), live on ALGO only: each amount ramps from the
        // last block's, and the engine captures the ops some ramp reads.
        let (from, to) = if m.engine() == EngineType::Algo {
            let to = m.taps.amounts();
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

        // A stage glides when no route moves its settings at audio rate:
        // then only a knob does, in steps (ADR 0117). Asked only of a
        // moving stage: the scans cost a steady block nothing.
        let routed = |block, ps: [ParamId; 3]| {
            ps.iter()
                .any(|&q| mod_state.moves_addr(ParamAddr::new(block, q)))
        };
        let drive_glide = || {
            !routed(
                BlockRef::Drive,
                [DriveParams::DRIVE, DriveParams::TONE, DriveParams::MIX],
            )
        };
        let fold_glide = || {
            !routed(
                BlockRef::Folder,
                [
                    FolderParams::FOLD,
                    FolderParams::SYMMETRY,
                    FolderParams::MIX,
                ],
            )
        };

        // 2. Drive, gated as it's billed.
        self.drive
            .process_glide(output, &m.drive, drive_on, drive_glide);

        // 3. Filter
        let mods = FilterMods {
            level_routed: mod_state
                .moves_addr(ParamAddr::new(BlockRef::Filter, FilterParams::LEVEL)),
            freq: lane(FixedCol::Freq),
            q: lane(FixedCol::Q),
            // A route moves g and k at audio rate; without one, only a
            // knob's glide does.
            glide: !mod_state.filter_routed(),
        };
        self.filter_clip = self.filter.process(output, &m.filter, sample_rate, &mods);

        // 4. Wavefolder
        self.folder.process_glide(
            output,
            &m.folder,
            fold_on,
            lane(FixedCol::Fold).as_ref(),
            fold_glide,
        );

        // 5. The tap-VCA, AM to ring, before the blocker: a tap correlated
        // with the signal leaves a DC, which the blocker takes.
        if let Some(l) = lane(FixedCol::Vca) {
            tap_vca(output, &l);
        }

        // 6. DC from the engine, the drive, the filter, the fold and the
        // tap-VCA: none reaches the VCA (ADR 0060).
        self.dc.run(output);

        if gated {
            self.gate.apply(output, self.held);
        }

        // 7. The VCA, after the blocker, with the engine's output gain,
        // ramped (never snap).
        let v = self.volume.step(m.out.volume * self.slot.out_gain());
        let (volume, dv) = (v.1, step_of(v, BLOCK_SIZE));
        // The block's last routed gain, × AMP's VEL: whether the lifetime
        // check below ends the voice through a fade.
        let mut last_gain = 0.0;
        match &gain {
            None => {
                // No route: the engine decides (spec § 4). No wildcard, so a new
                // engine can't inherit the pass-through (VA gates: #148).
                match &self.slot {
                    // TAPE's key ends it (`key_gates`), through the fade.
                    EngineSlot::Algo(_) | EngineSlot::Modal(_) | EngineSlot::Tape(_) => {
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
                let vel = 1.0 - m.out.vca_vel + m.out.vca_vel * self.last_velocity.unit();
                let k = volume * vel;
                #[allow(clippy::manual_clamp)] // max/min, not clamp: NaN clamps too
                if dv == 0.0 {
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

        // A TAPE note with no route into VCA ends at key up, through the fade.
        if self.active && self.fade == 0 && self.key_gates() && !self.held {
            self.after_fade = AfterFade::Idle;
            self.fade = Self::FADE;
        }

        // Lifetime by the VCA's routes (spec § 4), once per block: a voice
        // no routed source holds ends, through the fade if it still sounds.
        if self.active && self.fade == 0 && self.vca.bits != 0 && !self.vca_holds() {
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
