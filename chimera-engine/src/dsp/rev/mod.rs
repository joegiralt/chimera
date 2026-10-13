//! The master reverb TYPEs (reverb TYPEs spec, ADR 0085): `Reverb` runs
//! the half-band rate change, SMEAR's source and depth, and MIX, and
//! dispatches the block to the TYPE's engine over the one shared arena. A
//! TYPE change runs the switch (§ Switching TYPE): the old tail fades out
//! on its latched knobs, four blocks clear the arena a quarter each, and
//! the new TYPE fades in as a fresh engine.

pub mod arena;
pub mod bloom;
pub mod burst;
pub mod loop_engine;
pub mod parts;
pub mod room;
pub mod sync;

use core::mem::MaybeUninit;

use chimera_hal::BLOCK_SIZE;

use crate::dsp::Stereo;
use crate::dsp::clampf;
use crate::dsp::ease::{Ease, Gate, at, ease_coeff, step_of};
use crate::dsp::halfband::{Decimator, HALF, Interpolator};
use crate::dsp::reverb::{ReverbParams, ReverbType, knobs};
use crate::dsp::ring::{FULL_SCALE, STAGES, WET_GAIN, smear_depth, smear_hz};
use crate::hw::Cost;
use arena::ReverbArena;
use bloom::Bloom;
use burst::BurstEngine;
use loop_engine::{Hall, Layout, LoopEngine};
use parts::{Wander, wander_seeds};
use room::Room;
use sync::{FoldState, Tempo};

pub use crate::dsp::ring::QuadState;

/// The running TYPE's state. Zero bytes are `Quad(QuadState::default())`:
/// tag 0 is `Quad`, and every field of every variant is an integer, a
/// float or a `bool`.
#[repr(C, u8)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EngineState {
    Quad(QuadState),
    /// GATE and REVERSE.
    Burst(BurstEngine),
    Room(Room),
    Hall(Hall),
    Bloom(Bloom),
}

/// Each TYPE's return gain. ROOM's, HALL's, GATE's and REVERSE's match
/// QUAD's short-term loudness on a held −12 dBFS pink pad at MIX 0.5 (the
/// loudest 400 ms), their peaks within 1 dB of QUAD's: a TYPE switch under
/// a held note neither jumps nor clips (`check_sustained_loudness`).
/// ROOM's four loops at +1 and sixteen taps take 5.5 dB under QUAD's 4.54
/// (ADR 0095); HALL, through one loop of taps normalised to 3's level,
/// 9.7 dB over (1.2 dB under QUAD's, its peak within 1 dB of QUAD's, so
/// every preset golden stays under full scale).
pub fn wet_gain(t: ReverbType) -> f32 {
    match t {
        ReverbType::Room => WET_ROOM,
        ReverbType::Hall => WET_HALL,
        ReverbType::Gate => WET_GATE,
        ReverbType::Reverse => WET_REVERSE,
        ReverbType::Bloom => WET_BLOOM,
        ReverbType::Quad => WET_GAIN,
    }
}

pub(crate) const WET_ROOM: f32 = 2.42;
const WET_HALL: f32 = 14.0;
const WET_GATE: f32 = 9.0;
const WET_REVERSE: f32 = 4.84;
const WET_BLOOM: f32 = 5.72;

/// Each TYPE's REV row, cycles a sample: QUAD's bus row, the others it plus
/// the bench's extra over QUAD (GATE 96, REVERSE 112, ROOM 193, HALL 201,
/// BLOOM 206).
// measured 2026-10-09, bench, rev V at 480 MHz
pub const fn bench_cost(t: ReverbType) -> Cost {
    Cost(match t {
        ReverbType::Quad => 73,
        ReverbType::Gate => 169,
        ReverbType::Reverse => 185,
        ReverbType::Room => 266,
        ReverbType::Hall => 274,
        ReverbType::Bloom => 279,
    })
}

/// What a TYPE costs over QUAD, whose row `FxBus::COST` already holds: the
/// voice budget reserves it.
pub const fn extra_cost(t: ReverbType) -> Cost {
    Cost(bench_cost(t).0 - bench_cost(ReverbType::Quad).0)
}

/// What a TYPE switch's worst block costs over both TYPEs' extras: the
/// bench's REV SWITCH MAX (461,742 cycles) less STRESS's QUAD block
/// (433,347), a sample's share, less HALL's and BLOOM's extras then (188 + 194;
/// now 201 + 206, so 62 is a little over).
pub const SWITCH_COST: Cost = Cost(62);

/// Blocks a switch bills both TYPEs: its three phases and two to spare.
pub const SWITCH_BILLED: u8 = FADE_BLOCKS + CLEAR_BLOCKS + FADE_BLOCKS + 2;

/// The reverb's reserve from the voice budget, one `step` a block: the set
/// TYPE's extra over QUAD, and while a switch runs (the old TYPE fading out
/// beside the new, the clear) both extras and the switch's own, so the
/// allocator sheds a voice rather than run the block past its budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RevBill {
    from: ReverbType,
    to: ReverbType,
    left: u8,
}

impl RevBill {
    pub const fn new() -> Self {
        Self {
            from: ReverbType::Quad,
            to: ReverbType::Quad,
            left: 0,
        }
    }

    /// This block's reserve for the set TYPE `set`.
    pub fn step(&mut self, set: ReverbType) -> Cost {
        if set != self.to {
            // A switch on a switch: the costlier of the two still running.
            if self.left == 0 || extra_cost(self.to) > extra_cost(self.from) {
                self.from = self.to;
            }
            self.to = set;
            self.left = SWITCH_BILLED;
        }
        if self.left == 0 {
            return extra_cost(self.to);
        }
        self.left -= 1;
        extra_cost(self.from) + extra_cost(self.to) + SWITCH_COST
    }
}

impl Default for RevBill {
    fn default() -> Self {
        Self::new()
    }
}

/// ROOM's four loops (ADR 0095) set it: their gains, targets, dampers and
/// four smear offsets, 84 B past ADR 0085's 160, and GRAIN's envelope,
/// seed and its scale and knee, cached and eased, 24 more (112 with the
/// tests' probes).
pub const ENGINE_STATE_MAX: usize = 280;
const _: () = assert!(core::mem::size_of::<EngineState>() <= ENGINE_STATE_MAX);

/// Blocks each fade of the switch takes: 20 ms at 48 kHz.
pub const FADE_BLOCKS: u8 = 15;

/// Blocks CLEAR takes, a quarter of the arena each: no block costs more
/// than the engine work it replaces.
pub const CLEAR_BLOCKS: u8 = 4;

/// Blocks a phase has left, 1..=`N`: a ramp can't run past either end.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Left<const N: u8>(u8);

impl<const N: u8> Left<N> {
    /// The phase's first block.
    pub const FULL: Self = Self(N);

    /// `n` blocks left, if `n` is 1..=`N`.
    pub const fn new(n: u8) -> Option<Self> {
        if n >= 1 && n <= N {
            Some(Self(n))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    /// Blocks of the phase already run, 0..`N`.
    pub const fn done(self) -> u8 {
        N - self.0
    }

    /// One block on; `None` past the last.
    const fn next(self) -> Option<Self> {
        Self::new(self.0 - 1)
    }
}

/// Where a TYPE switch is: the phase the next block runs. Zero bytes are
/// `Running`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchPhase {
    Running,
    /// The old TYPE on its latched knobs, its return ramping to 0: `left`
    /// blocks of it, the ramp at `left / FADE_BLOCKS`.
    FadeOut {
        left: Left<FADE_BLOCKS>,
    },
    /// No engine, the return exactly 0: quarter `left.done()` of the arena
    /// zeroed; with the last, the state and the half-bands.
    Clear {
        left: Left<CLEAR_BLOCKS>,
    },
    /// The new TYPE, fresh, its return ramping from 0: `left` blocks of it.
    FadeIn {
        left: Left<FADE_BLOCKS>,
    },
}

impl SwitchPhase {
    /// This block's return ramp `(from, to)`; `None` without one.
    pub(crate) fn gains(self) -> Option<(f32, f32)> {
        let g = |n: u8| f32::from(n) / f32::from(FADE_BLOCKS);
        match self {
            Self::FadeOut { left } => Some((g(left.get()), g(left.get() - 1))),
            Self::FadeIn { left } => Some((g(left.done()), g(left.done() + 1))),
            Self::Running | Self::Clear { .. } => None,
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Running => Self::Running,
            Self::FadeOut { left } => left
                .next()
                .map_or(Self::Clear { left: Left::FULL }, |left| Self::FadeOut {
                    left,
                }),
            Self::Clear { left } => left
                .next()
                .map_or(Self::FadeIn { left: Left::FULL }, |left| Self::Clear {
                    left,
                }),
            Self::FadeIn { left } => left
                .next()
                .map_or(Self::Running, |left| Self::FadeIn { left }),
        }
    }
}

pub struct Reverb {
    arena: ReverbArena,
    /// The TYPE `state` runs.
    kind: ReverbType,
    state: EngineState,
    phase: SwitchPhase,
    /// The knobs the engine runs: each block's, until a switch starts;
    /// then held, so FADE OUT never plays the new TYPE's.
    latched: ReverbParams,
    dec: Decimator,
    up_l: Interpolator,
    up_r: Interpolator,
    /// SMEAR's knob, eased (never snap).
    smear: Ease,
    /// SMEAR's depth on or resting: it rests, linearly over 20 ms, while
    /// a SIZE move waits or crossfades.
    smear_on: Gate,
    /// One wander per smeared line.
    wander: [Wander; STAGES],
    /// The RATE (its bits) the wanders were last set for, once
    /// `wanders_set`.
    wanders_rate: u32,
    wanders_set: bool,
    /// The wanders held at their centre (tests only).
    frozen: bool,
    /// SMEAR's depth snaps off and on, with no 20 ms gate (the click
    /// test's negative control): never in firmware.
    #[cfg(any(test, feature = "smear-probe"))]
    hard_gate: bool,
    /// Last block's (SIZE crossfade, SMEAR) paths, and its end's depth.
    paths: (bool, bool),
    depth: f32,
    /// MIX, eased (never snap).
    mix: Ease,
    /// The synced time's fold, held across blocks (§ Tempo sync).
    fold: FoldState,
    /// The tail's last LSBs faded out.
    tail: TailGate,
}

#[cfg(any(test, feature = "smear-probe"))]
crate::in_place::field_list!(Reverb => Reverb {
    arena, kind, state, phase, latched, dec, up_l, up_r, smear, smear_on, wander, wanders_rate, wanders_set, frozen, hard_gate, paths, depth, mix, fold, tail
});
#[cfg(not(any(test, feature = "smear-probe")))]
crate::in_place::field_list!(Reverb => Reverb {
    arena, kind, state, phase, latched, dec, up_l, up_r, smear, smear_on, wander, wanders_rate, wanders_set, frozen, paths, depth, mix, fold, tail
});

/// QUAD's SMEAR seed: its four lines' wanders hash it.
const QUAD_SEED: u32 = 0x5EED_0343;

/// QUAD's wanders, line k starting k/4 of a period in: built at compile
/// time, so a reset copies them.
const QUAD_WANDERS: [Wander; STAGES] = {
    let seeds = wander_seeds::<STAGES>(QUAD_SEED);
    let mut w = [Wander::new(0, 0.0); STAGES];
    let mut k = 0;
    while k < STAGES {
        w[k] = Wander::new(seeds[k], k as f32 / STAGES as f32);
        k += 1;
    }
    w
};

impl Reverb {
    pub fn new() -> Self {
        Self {
            arena: ReverbArena::new(),
            kind: ReverbType::Quad,
            state: EngineState::Quad(QuadState::default()),
            phase: SwitchPhase::Running,
            latched: ReverbParams::default(),
            dec: Decimator::new(),
            up_l: Interpolator::new(),
            up_r: Interpolator::new(),
            smear: Ease::default(),
            smear_on: Gate::default(),
            wander: QUAD_WANDERS,
            wanders_rate: 0,
            wanders_set: false,
            frozen: false,
            #[cfg(any(test, feature = "smear-probe"))]
            hard_gate: false,
            paths: (false, false),
            depth: 0.0,
            mix: Ease::default(),
            fold: FoldState(0),
            tail: TailGate::new(),
        }
    }

    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        // SAFETY: the arena and half-bands are integer and float arrays and
        // integers; `tail` floats and a `bool`; `state` is `EngineState`, whose
        // variant 0 is `Quad` and every field of every variant an integer,
        // a float or a `bool` (`false`); `kind` and `phase` are `repr(u8)` enums whose 0 is
        // `Quad` and `Running`, `latched` floats and two `repr(u8)` enums
        // (0 is `Quad`, `Off`); `smear` and `mix` are `Ease`s of a float
        // and a `bool`, `smear_on` a `Gate` of a `u8` and a `bool`,
        // `wander` integers and floats, `fold` an `i8`, the rest integers
        // and `bool`s. All are valid as zero bytes, and zero is `new()`'s
        // state but for the
        // wanders' seeds and phases, set below, and `latched`'s knobs,
        // which a block writes before it reads them unless the first
        // block already switches: that FADE OUT plays an empty arena.
        let r = unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        };
        r.wander = QUAD_WANDERS;
        r
    }

    /// Take the tail gate out of the path from now on: the RT60 tests read
    /// the engine's own decay, not the gate's last 35 dB.
    #[cfg(any(test, feature = "test-support"))]
    pub fn tail_gate_off(&mut self) {
        self.tail.off = true;
    }

    /// Hold every wander at its centre from now on: SMEAR's reads sit `D`
    /// short, still. The pitch reference for `check_smear_bound`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn freeze_smear(&mut self) {
        self.frozen = true;
    }

    /// Snap SMEAR's depth to 0 for a SIZE move, and back, in one block:
    /// what the gate prevents, for the click test to catch.
    #[cfg(any(test, feature = "smear-probe"))]
    pub fn hard_smear_gate(&mut self) {
        self.hard_gate = true;
    }

    /// Last block's paths: (a SIZE crossfade ran, SMEAR moved the reads).
    pub fn debug_paths(&self) -> (bool, bool) {
        self.paths
    }

    /// SMEAR's depth D at last block's end, in ring samples.
    pub fn debug_depth(&self) -> f32 {
        self.depth
    }

    pub fn arena(&self) -> &ReverbArena {
        &self.arena
    }

    /// The synced time's fold as it plays: what its cell marks.
    pub fn fold(&self) -> FoldState {
        self.fold
    }

    /// Where a TYPE switch is.
    pub fn phase(&self) -> SwitchPhase {
        self.phase
    }

    /// The TYPE the engine runs: the old one until CLEAR.
    pub fn running(&self) -> ReverbType {
        self.kind
    }

    /// A fresh reverb running `kind`, as CLEAR leaves it.
    #[cfg(any(test, feature = "test-support"))]
    pub fn of_type(kind: ReverbType) -> Self {
        let mut r = Self::new();
        r.clear(kind);
        r
    }

    /// Start a switch, as a TYPE change does, to whatever TYPE is set at
    /// CLEAR: from RUNNING, FADE OUT from 1; from FADE IN, FADE OUT from
    /// the gain it reached (from none, straight to CLEAR); in FADE OUT or
    /// CLEAR, the switch runs on. The bench's REV SWITCH MAX and the tests
    /// run QUAD → QUAD by it.
    #[doc(hidden)]
    pub fn force_restart(&mut self) {
        self.phase = match self.phase {
            SwitchPhase::Running => SwitchPhase::FadeOut { left: Left::FULL },
            SwitchPhase::FadeIn { left } => Left::new(left.done())
                .map_or(SwitchPhase::Clear { left: Left::FULL }, |left| {
                    SwitchPhase::FadeOut { left }
                }),
            s => s,
        };
    }

    /// All of CLEAR at once: the arena, then `reset`.
    #[cfg(any(test, feature = "test-support"))]
    fn clear(&mut self, kind: ReverbType) {
        self.arena.clear();
        self.reset(kind);
    }

    /// CLEAR's last block: with the arena zeroed, no sample of the old
    /// tail survives. `kind`'s fresh state, SMEAR's eases and wanders, and
    /// the half-bands' histories: the decimator's too, so the new TYPE is
    /// bit for bit a fresh engine fed the send from FADE IN's first block
    /// (the send CLEAR drops would leave a gap in it anyway). MIX keeps
    /// easing: it is the player's.
    fn reset(&mut self, kind: ReverbType) {
        self.kind = kind;
        self.state = match kind {
            ReverbType::Gate | ReverbType::Reverse => EngineState::Burst(BurstEngine::default()),
            ReverbType::Room => EngineState::Room(Room::default()),
            ReverbType::Hall => EngineState::Hall(Hall::default()),
            ReverbType::Bloom => EngineState::Bloom(Bloom::default()),
            ReverbType::Quad => EngineState::Quad(QuadState::default()),
        };
        self.dec = Decimator::new();
        self.up_l = Interpolator::new();
        self.up_r = Interpolator::new();
        self.smear = Ease::default();
        self.smear_on = Gate::default();
        self.wander = QUAD_WANDERS;
        (self.wanders_rate, self.wanders_set) = (0, false);
        (self.paths, self.depth) = ((false, false), 0.0);
        self.fold = FoldState(0);
        self.tail = TailGate::new();
    }

    /// QUAD's state, while QUAD runs.
    pub fn quad(&self) -> Option<&QuadState> {
        match &self.state {
            EngineState::Quad(q) => Some(q),
            _ => None,
        }
    }

    /// BURST's state, while GATE or REVERSE runs.
    pub fn burst(&self) -> Option<&BurstEngine> {
        match &self.state {
            EngineState::Burst(b) => Some(b),
            _ => None,
        }
    }

    /// ROOM's state to change, while ROOM runs (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn room_mut(&mut self) -> Option<&mut Room> {
        match &mut self.state {
            EngineState::Room(e) => Some(e),
            _ => None,
        }
    }

    /// ROOM's state, while ROOM runs.
    pub fn room(&self) -> Option<&Room> {
        match &self.state {
            EngineState::Room(e) => Some(e),
            _ => None,
        }
    }

    /// BLOOM's state to change, while BLOOM runs (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn bloom_mut(&mut self) -> Option<&mut Bloom> {
        match &mut self.state {
            EngineState::Bloom(e) => Some(e),
            _ => None,
        }
    }

    /// BLOOM's state, while BLOOM runs.
    pub fn bloom(&self) -> Option<&Bloom> {
        match &self.state {
            EngineState::Bloom(e) => Some(e),
            _ => None,
        }
    }

    /// HALL's state to change, while HALL runs (tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn hall_mut(&mut self) -> Option<&mut Hall> {
        match &mut self.state {
            EngineState::Hall(e) => Some(e),
            _ => None,
        }
    }

    /// HALL's state, while HALL runs.
    pub fn hall(&self) -> Option<&Hall> {
        match &self.state {
            EngineState::Hall(e) => Some(e),
            _ => None,
        }
    }

    /// Wet only: the engine's return × `mix`, into `out`. A synced time
    /// follows `tempo`; QUAD runs free of it.
    pub fn process(
        &mut self,
        send: &[f32; BLOCK_SIZE],
        p: &ReverbParams,
        mix: f32,
        tempo: Tempo,
        sample_rate: u32,
        out: &mut Stereo,
    ) {
        let switching = matches!(
            self.phase,
            SwitchPhase::FadeOut { .. } | SwitchPhase::Clear { .. }
        );
        if !switching {
            if p.kind != self.kind {
                self.force_restart();
            } else {
                self.latched = *p;
            }
        }
        let phase = self.phase;
        self.phase = phase.next();
        let (from, to) = self.mix.step(mix, ease_coeff(sample_rate));
        if let SwitchPhase::Clear { left } = phase {
            // The arena isn't read until FADE IN: a part-zeroed one is never heard.
            self.arena.clear_part(left.done(), CLEAR_BLOCKS);
            if left.get() == 1 {
                self.reset(p.kind);
            }
            out.l.fill(0.0);
            out.r.fill(0.0);
            return;
        }
        let p = &self.latched;
        // A row TYPE's knobs, mapped once a block (fast `exp2`/`log2`).
        let kn = match self.state {
            EngineState::Burst(_)
            | EngineState::Room(_)
            | EngineState::Hall(_)
            | EngineState::Bloom(_) => Some(knobs(p, tempo, &mut self.fold)),
            EngineState::Quad(_) => None,
        };
        let mut u = [0.0f32; HALF];
        self.dec.process(send, &mut u);
        let mut lr = [[0.0f32; HALF]; 2];
        let fs = sample_rate as f32 * 0.5;
        let rate = if p.rate.is_nan() {
            crate::dsp::reverb::RATE_DEFAULT
        } else {
            p.rate
        };
        if !self.wanders_set || self.wanders_rate != rate.to_bits() {
            (self.wanders_rate, self.wanders_set) = (rate.to_bits(), true);
            let hz = kn.map_or_else(|| smear_hz(rate), |k| k.rate_hz);
            let coefs = Wander::rate(hz, fs);
            for w in &mut self.wander {
                w.set(coefs);
            }
        }
        let x0 = self.wander.map(|w| w.value());
        let x1 = if self.frozen {
            x0
        } else {
            core::array::from_fn(|k| self.wander[k].block())
        };
        let knob = if p.smear.is_nan() {
            0.0
        } else {
            clampf(p.smear, 0.0, 1.0)
        };
        let (s0, s1) = self.smear.step(knob, ease_coeff(sample_rate));
        match &mut self.state {
            EngineState::Quad(q) => {
                let on = !q.wants_size(p.size);
                #[cfg(any(test, feature = "smear-probe"))]
                let (g0, g1) = if self.hard_gate {
                    (on as u8 as f32, on as u8 as f32)
                } else {
                    self.smear_on.step(on)
                };
                #[cfg(not(any(test, feature = "smear-probe")))]
                let (g0, g1) = self.smear_on.step(on);
                let full = smear_depth(1.0, q.ring().step());
                let (d0, d1) = (s0 * g0 * full, s1 * g1 * full);
                let smear = (d0 != 0.0 || d1 != 0.0)
                    .then(|| core::array::from_fn(|k| (d0 * (1.0 + x0[k]), d1 * (1.0 + x1[k]))));
                self.paths = (q.ring().crossfading(), smear.is_some());
                self.depth = d1;
                q.process(
                    &mut self.arena,
                    &u,
                    &p.controls(),
                    smear,
                    sample_rate,
                    &mut lr,
                );
                if q.ring().crossfading() {
                    self.paths.0 = true;
                }
            }
            EngineState::Burst(b) => {
                // Its two smeared decorrelators sit outside every loop:
                // no SIZE crossfade to rest for, no cap.
                let full = kn.map_or(0.0, |k| k.smear_full_ms) * burst::PER_MS;
                let (d0, d1) = (s0 * full, s1 * full);
                b.set_smear(core::array::from_fn(|k| {
                    (d0 * (1.0 + x0[k]), d1 * (1.0 + x1[k]))
                }));
                (self.paths, self.depth) = ((false, d0 != 0.0 || d1 != 0.0), d1);
                let [l, r] = &mut lr;
                b.run(&mut self.arena, &u, &kn.unwrap_or_default(), l, r);
            }
            EngineState::Room(e) => {
                // One wander a loop, its smeared allpass outside every
                // crossfade, as HALL's.
                let kn = kn.unwrap_or_default();
                let full = kn.smear_full_ms * room::PER_MS;
                let (d0, d1) = (s0 * full, s1 * full);
                e.set_smear(core::array::from_fn(|k| {
                    (d0 * (1.0 + x0[k]), d1 * (1.0 + x1[k]))
                }));
                self.paths = (e.crossfade().is_some(), d0 != 0.0 || d1 != 0.0);
                self.depth = d1;
                let [l, r] = &mut lr;
                e.run(&mut self.arena, &u, &kn, l, r);
            }
            EngineState::Bloom(e) => {
                // One wander a smeared allpass, the three in one loop.
                let kn = kn.unwrap_or_default();
                let full = kn.smear_full_ms * bloom::PER_MS;
                let (d0, d1) = (s0 * full, s1 * full);
                e.set_smear(core::array::from_fn(|k| {
                    (d0 * (1.0 + x0[k]), d1 * (1.0 + x1[k]))
                }));
                self.paths = (e.crossfade().is_some(), d0 != 0.0 || d1 != 0.0);
                self.depth = d1;
                let [l, r] = &mut lr;
                e.run(&mut self.arena, &u, &kn, l, r);
            }
            EngineState::Hall(e) => {
                (self.paths, self.depth) = run_loop(
                    e,
                    &mut self.arena,
                    &u,
                    &kn.unwrap_or_default(),
                    (s0, s1),
                    (x0, x1),
                    &mut lr,
                );
            }
        }
        // MIX's ease × the tail gate's, one ramp.
        let (g0, g1) = self.tail.step(&mut lr, self.kind);
        let (from, to) = (from * g0, to * g1);
        self.up_l.process(&lr[0], &mut out.l);
        self.up_r.process(&lr[1], &mut out.r);
        let wet = wet_gain(self.kind);
        let gain = wet * to / FULL_SCALE;
        if from == to {
            for s in out.l.iter_mut().chain(out.r.iter_mut()) {
                *s *= gain;
            }
        } else {
            ramp(
                out,
                wet * from / FULL_SCALE,
                step_of((from, to), BLOCK_SIZE) * wet / FULL_SCALE,
            );
        }
        // The switch's ramp: its own pass, so FADE IN is a fresh engine's
        // return times the ramp, bit for bit.
        if let Some(r) = phase.gains() {
            ramp(out, r.0, step_of(r, BLOCK_SIZE));
        }
    }
}

/// HALL's block: its two smeared allpasses' offsets at D =
/// `kn.smear_full_ms` × SMEAR's ease `s`, from the wanders' `x`, then the
/// engine. Its smeared allpasses never crossfade: no SIZE move to rest
/// for. The cap is in the knobs, and each loop's in the engine. Returns
/// the paths and the depth `Reverb` reports.
#[allow(clippy::too_many_arguments)]
fn run_loop<const L: usize, const S: usize>(
    e: &mut LoopEngine<L, S>,
    arena: &mut ReverbArena,
    u: &[f32; HALF],
    kn: &crate::dsp::reverb::EngineKnobs,
    (s0, s1): (f32, f32),
    (x0, x1): ([f32; STAGES], [f32; STAGES]),
    [l, r]: &mut [[f32; HALF]; 2],
) -> ((bool, bool), f32)
where
    LoopEngine<L, S>: Layout<L, S>,
{
    let full = kn.smear_full_ms * loop_engine::PER_MS;
    let (d0, d1) = (s0 * full, s1 * full);
    e.set_smear(core::array::from_fn(|k| {
        (d0 * (1.0 + x0[k]), d1 * (1.0 + x1[k]))
    }));
    let paths = (e.crossfade().is_some(), d0 != 0.0 || d1 != 0.0);
    e.run(arena, u, kn, l, r);
    (paths, d1)
}

/// Each TYPE's storage floor as `TailGate`'s envelope reads it (|l| + |r|
/// block peak, ring LSBs): the noise its i16 lines add while a tail
/// plays, measured as the 4–16 kHz band that scales up 12 dB when the
/// send drops 12 (QUAD −69, ROOM −74, HALL −67 dBFS in band at MIX 1), and
/// for BLOOM against a float-line render (2.3 LSB RMS a side: its 15
/// normalised allpasses each hand theirs back × 1/(1 − g)).
pub fn tail_floor(t: ReverbType) -> f32 {
    match t {
        ReverbType::Quad => 9.0,
        ReverbType::Room => 9.5,
        ReverbType::Hall => 4.0,
        ReverbType::Bloom => 11.0,
        ReverbType::Gate | ReverbType::Reverse => 4.0,
    }
}

/// Each TYPE's `TailGate` knee, |l| + |r| as `tail_floor`: where the
/// noise-only end fades (as first set, so the tail is no shorter).
pub fn tail_knee(t: ReverbType) -> f32 {
    match t {
        ReverbType::Bloom => 72.0,
        _ => 20.0,
    }
}

/// The return's last 35 dB over its storage floor (`tail_floor`): the
/// lines' noise is white, half of it over 4 kHz, where a damped tail has
/// nothing, so a dark late tail 25 dB over it already hisses. A two-pole
/// low-pass follows the tail down, its coefficient the envelope over
/// `OPEN` floors (open, untouched, above; about 1.5 kHz at the least),
/// so the tail's 4–16 kHz share stays within 9 dB of its body's; under
/// the knee (`tail_knee`) the return goes as the envelope's fourth power
/// (5:1), and the noise-only end fades. Both close on a ramp across the
/// block (the gain on MIX's). The low-pass opens at once, on a return
/// that was under the floor; the gain opens on a ramp too, at once only
/// on an onset. 19 instructions a ring sample (`FxBus::COST`). Zero bytes
/// are a fresh gate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TailGate {
    /// The block peak of |l| + |r|: instant up, falling by `RELEASE`.
    env: f32,
    /// Last block's gain.
    g: f32,
    /// Last block's low-pass coefficient, 1 open.
    a: f32,
    /// Each side's two low-pass states.
    y: [[f32; 2]; 2],
    /// Out of the path (tests: the engine's own decay).
    off: bool,
}

impl TailGate {
    /// The envelope's fall a block: τ 50 ms at 48 kHz.
    const RELEASE: f32 = 0.026;
    /// Where the low-pass opens fully: 35 dB over the floor.
    pub const OPEN: f32 = 56.0;
    /// The low-pass's lowest coefficient: about 1.5 kHz at 24 kHz, so the
    /// tail's own body is no shorter.
    const A_MIN: f32 = 0.32;
    /// An onset: the block's peak over last block's envelope by this
    /// (18 dB).
    const ONSET: f32 = 8.0;

    pub const fn new() -> Self {
        Self {
            env: 0.0,
            g: 0.0,
            a: 0.0,
            y: [[0.0; 2]; 2],
            off: false,
        }
    }

    /// The gain at envelope `env`: 1 from `knee`, (env / `knee`)⁴ under.
    pub fn gain(env: f32, knee: f32) -> f32 {
        let x = (env / knee).min(1.0);
        let x2 = x * x;
        x2 * x2
    }

    /// The low-pass coefficient at envelope `e` over the floor: 1 from
    /// `OPEN`, e / `OPEN` under, `A_MIN` at least.
    pub fn coef(e: f32) -> f32 {
        clampf(e * (1.0 / Self::OPEN), Self::A_MIN, 1.0)
    }

    /// Low-passes the return `lr` in place and gives this block's gain
    /// ramp `(from, to)`.
    pub fn step(&mut self, lr: &mut [[f32; HALF]; 2], t: ReverbType) -> (f32, f32) {
        if self.off {
            return (1.0, 1.0);
        }
        let [l, r] = lr;
        let pk = l
            .iter()
            .zip(r.iter())
            .fold(0.0f32, |m, (a, b)| m.max(a.abs() + b.abs()));
        let prev = self.env;
        self.env = if pk > self.env {
            pk
        } else {
            self.env + Self::RELEASE * (pk - self.env)
        };
        // Opening is instant (a hit after a gap keeps its edge: what came
        // before it was under the floor); closing ramps across the block.
        let a1 = Self::coef(self.env / tail_floor(t));
        let a0 = if a1 > self.a { a1 } else { self.a };
        self.a = a1;
        if a0 == 1.0 && a1 == 1.0 {
            // Open: the state follows, so closing starts where it is.
            self.y = [[l[HALF - 1]; 2], [r[HALF - 1]; 2]];
        } else {
            let da = step_of((a0, a1), HALF);
            let [mut yl, mut yr] = self.y;
            for (i, (x, z)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                let a = at(a0, da, i);
                yl[0] += a * (*x - yl[0]);
                yl[1] += a * (yl[0] - yl[1]);
                yr[0] += a * (*z - yr[0]);
                yr[1] += a * (yr[0] - yr[1]);
                (*x, *z) = (yl[1], yr[1]);
            }
            self.y = [yl, yr];
        }
        // The gain ramps up as down: snapped, a tail under the knee
        // stepped on each block edge its peak rose (#499). Only an onset
        // snaps it, its edge sample at most last block's envelope, an
        // eighth of the block's own swing.
        let g1 = Self::gain(self.env, tail_knee(t));
        let g0 = if pk > Self::ONSET * prev { g1 } else { self.g };
        self.g = g1;
        (g0, g1)
    }
}

impl Default for TailGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Both sides × a ramp from `g0` by `step` a sample: MIX's, and the
/// switch's. Out of line: one copy of the loop for both.
#[inline(never)]
fn ramp(out: &mut Stereo, g0: f32, step: f32) {
    for (i, (l, r)) in out.l.iter_mut().zip(out.r.iter_mut()).enumerate() {
        let g = at(g0, step, i);
        (*l, *r) = (*l * g, *r * g);
    }
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new()
    }
}
