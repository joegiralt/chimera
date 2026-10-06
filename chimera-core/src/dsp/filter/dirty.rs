//! DIRTY: the Chamberlin state-variable filter (spec § 4), run twice a
//! sample with the input held. Cheap, digital and edgy near the top: no
//! divide, no saturator, only hard clamps.
//!
//! **Why it can't blow up.** One sub-step's matrix `[[1, f], [−f, 1 − fq −
//! f²]]` is stable (Jury) exactly when `0 < fq` and `f < √(q² + 4) − q`.
//! `f` is clamped once a block to 0.95 of that at the block's largest `q`,
//! and ×2 halves `f` for a given FREQ, so the ceiling at RES 0 is 12.35 kHz
//! and rises with RES. The stable region isn't convex, so a ramp may leave
//! it for a sample; the clamps on `lp`, `bp` and the output bound every
//! state and sample for any input, `f`, `q`, ramp or tap.

use super::level::{FilterMods, LevelRamp, LevelStage, level_gain};
use super::{FilterMode, TapShape, ramps};
use crate::dsp::{clampf, fast_sin};
use crate::hw::{BLOCK_SIZE, Cost};
use crate::meter::Clip;
use crate::params::FilterParams;

/// DIRTY's modes, in knob order (spec § 1): LP2 the default. The index is
/// `run::<M>`'s.
pub(crate) const DIRTY_MODES: [FilterMode; 4] = [
    FilterMode::Lp12,
    FilterMode::Bp12,
    FilterMode::Hp12,
    FilterMode::Notch,
];

/// The states' clamp, `lp` and `bp ∈ ±2`.
pub const DIRTY_STATE: f32 = 2.0;
/// The output gain: ½ on the sub-steps' mean (M5). Without it DIRTY runs
/// about 6 dB hot of the other types.
pub const DIRTY_OUT: f32 = 0.5;
/// The output's clamp, ±1.
pub const DIRTY_PEAK: f32 = 1.0;
/// The share of Jury's bound `f` may take.
pub const DIRTY_MARGIN: f32 = 0.95;

/// DIRTY's bill over `OLD_LP24`, any mode: 0. Counted at 27 (HP2) to 29
/// (NOTCH1) instructions a sample, 0 `vdiv`: `ceil5(29 × 1.606)` = 50
/// cycles, 55 with the block's 3.6, under LP24's 95 (plan § Measured, Task
/// 9). One loop serves steady and moving blocks.
pub const DIRTY_COST: Cost = Cost(0);

/// RES's `q`: 2 at RES 0, 0 at 0.95 (the onset every type shares) and
/// −0.105 at RES 1, where DIRTY oscillates into its clamps. RES is held to
/// 0..1 first, a NaN to 0, so a bad value can't poison the next block's
/// ramp.
#[inline(always)]
#[allow(clippy::manual_clamp)] // `clamp` passes a NaN; this lands it on 0
pub fn dirty_q(res: f32) -> f32 {
    let r = res.max(0.0).min(1.0);
    2.0 * (1.0 - r * (1.0 / 0.95))
}

/// `mode`'s output trim at `q`, as CLEAN's `trim` at `k`: 1 for `q ≥ ½`,
/// linear to its trim at `q ≤ 0`, which brings the clamped ring's middle
/// over 100 Hz – 8 kHz to −8 dBFS, the middle of the shared band
/// (`OSC_BAND_DB`), so a TYPE switch at RES 1 doesn't jump. Ramped across
/// the block with the output gain: one add a sample.
#[inline(always)]
pub fn dirty_trim(mode: FilterMode, q: f32) -> f32 {
    // Untrimmed at RES 1, dBFS.
    let osc = match mode {
        FilterMode::Lp12 => DIRTY_TRIM_LP2, // −2.76 to −3.00
        FilterMode::Bp12 => DIRTY_TRIM_BP2, // −2.75 to −2.95
        FilterMode::Hp12 => DIRTY_TRIM_HP2, // −2.76 to −2.97
        // NOTCH1's zero sits on the ring's pitch (−22.3 to −22.7): untrimmed,
        // as CLEAN's NOTCH2, which never rings.
        FilterMode::Notch => 1.0,
        // CLEAN's; `DIRTY_MODES` holds none of them.
        FilterMode::Lp6
        | FilterMode::Lp24
        | FilterMode::Bp24
        | FilterMode::Hp24
        | FilterMode::Phaser => 1.0,
    };
    let u = clampf(1.0 - 2.0 * q, 0.0, 1.0);
    1.0 + (osc - 1.0) * u
}

/// The trims at `q ≤ 0`: `10^((−8 − mid)/20)`, `mid` the mode's untrimmed
/// RES 1 range's middle (plan § Measured, Task 9 fix round).
pub const DIRTY_TRIM_LP2: f32 = 0.5546;
pub const DIRTY_TRIM_BP2: f32 = 0.5527;
pub const DIRTY_TRIM_HP2: f32 = 0.5537;

/// FREQ's `f` at ×2 before the bound, `2·sin(π·fc/(2·fs))`: exact pitch at
/// `q = 0`. `fc/fs` is held to 0..0.497 as CLEAN's `g` is; a NaN lands on
/// 0.497.
#[inline(always)]
#[allow(clippy::manual_clamp)] // `clamp` passes a NaN; this lands it on 0.497
pub fn dirty_f_raw(fc: f32, fs: f32) -> f32 {
    let r = (fc / fs).min(0.497).max(0.0);
    2.0 * fast_sin(core::f32::consts::FRAC_PI_2 * r)
}

/// The largest `f` at `q`: `0.95·(√(q² + 4) − q)`, Jury's bound with a
/// margin. One `vsqrt` a block.
#[inline(always)]
pub fn dirty_f_bound(q: f32) -> f32 {
    DIRTY_MARGIN * (libm::sqrtf(q * q + 4.0) - q)
}

/// The block's `f` at FREQ `fc`, bounded at the block's largest `q`.
#[inline(always)]
pub fn dirty_f(fc: f32, fs: f32, q_max: f32) -> f32 {
    dirty_f_raw(fc, fs).min(dirty_f_bound(q_max))
}

/// Two states, the last block's ramp ends (`None` on a fresh filter, which
/// starts unramped) and LEVEL's clipper ahead of the loop, as on CLEAN.
#[derive(Clone, Debug, Default)]
pub struct Dirty {
    lp: f32,
    bp: f32,
    /// One `Option` for the three: they are primed and forgotten together,
    /// and it is 8 B under three (ADR 0101's 2,112 B voice chain).
    last: Option<Ends>,
    level: LevelStage,
}

/// A block's end values of `f`, `q` and LEVEL's gain.
#[derive(Clone, Copy, Debug)]
struct Ends {
    f: f32,
    q: f32,
    pre: f32,
}

/// One block's `(from, to)` for `f` and `q`.
#[derive(Clone, Copy)]
pub(super) struct Ramps {
    f: (f32, f32),
    q: (f32, f32),
    /// Jury's bound at the largest `q` the block can reach: a tapped
    /// sample's `f` is held under it.
    f_max: f32,
}

impl Dirty {
    pub fn new() -> Self {
        Self::default()
    }

    /// One block through LEVEL and the mode: LEVEL's `clip_amount` for the
    /// LED.
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        p: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> Clip {
        let r = self.prelude(buf, p, fs, mods);
        let t = TapShape::new(mods, (dirty_f_raw(20.0, fs as f32), r.f_max), dirty_q);
        let t = t.as_ref();
        let sum = match p.mode() {
            FilterMode::Bp12 => self.run::<1>(buf, r, t),
            FilterMode::Hp12 => self.run::<2>(buf, r, t),
            FilterMode::Notch => self.run::<3>(buf, r, t),
            // LP2, and any of CLEAN's modes, which `set_mode` keeps off a
            // DIRTY Sound.
            _ => self.run::<0>(buf, r, t),
        };
        self.settle(sum);
        self.level.clip()
    }

    /// This block's ramps, stored as the next block's start, and LEVEL's
    /// stage run on `buf`. `f` is bounded at the larger `q` end, so every
    /// sample's `q` is at most the one it was bounded at.
    #[inline(always)]
    pub(super) fn prelude(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        p: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> Ramps {
        let (q, pre) = (dirty_q(p.resonance), level_gain(p.level));
        let from = self.last.map_or((q, pre), |e| (e.q, e.pre));
        // A Q tap moves RES by up to its reach, so `q` by `2/0.95` that.
        let reach = mods.q.map_or(0.0, |l| l.reach() * (2.0 / 0.95));
        let q_max = (from.0.max(q) + reach).min(dirty_q(0.0));
        let f = dirty_f(p.cutoff, fs as f32, q_max);
        let f0 = self.last.map_or(f, |e| e.f);
        self.last = Some(Ends { f, q, pre });
        self.level.run(buf, LevelRamp::new((from.1, pre), mods));
        Ramps {
            f: (f0, f),
            q: (from.0, q),
            f_max: dirty_f_bound(q_max),
        }
    }

    /// A NaN or infinite input: the clamps hold the states finite, but at
    /// a rail, so start the next block from rest. `sum` is the block's
    /// inputs summed, not finite exactly when one was not.
    pub(super) fn settle(&mut self, sum: f32) {
        if !sum.is_finite() {
            (self.lp, self.bp) = (0.0, 0.0);
        }
    }

    /// Mode `M` (`DIRTY_MODES`' index) over the block, `f` and `q` ramped
    /// by adding: a steady block's steps are 0, so one loop serves both. A
    /// FREQ or Q tap moves each sample's (`TapShape`), `f` held under
    /// Jury's bound at the reachable `q`; the trim stays on the ramp's.
    /// Returns the inputs' sum (`settle`).
    #[inline(always)]
    pub(super) fn run<const M: u8>(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        tap: Option<&TapShape>,
    ) -> f32 {
        let n = BLOCK_SIZE as f32;
        let (f_step, q_step) = ((r.f.1 - r.f.0) / n, (r.q.1 - r.q.0) / n);
        let (mut fs, mut qs) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
        ramps(&mut fs, &mut qs, (r.f.0, f_step), (r.q.0, q_step), true);
        if let Some(t) = tap {
            t.apply(&mut fs, &mut qs);
        }
        // The output gain, ½ of the mean × the mode's trim, ramped with `q`.
        let mode = DIRTY_MODES[M as usize];
        let gain = |q| 0.5 * DIRTY_OUT * dirty_trim(mode, q);
        let (mut g, g1) = (gain(r.q.0), gain(r.q.1));
        let g_step = (g1 - g) / n;
        let mut sum = 0.0f32;
        for (i, s) in buf.iter_mut().enumerate() {
            g += g_step;
            let (f, q) = (fs[i], qs[i]);
            let x = *s;
            sum += x;
            let m = self.sub::<M>(x, f, q) + self.sub::<M>(x, f, q);
            *s = clampf(g * m, -DIRTY_PEAK, DIRTY_PEAK);
        }
        sum
    }

    /// One sub-step at ×2: Chamberlin's update, then both states clamped
    /// (spec § 4's order: `hp` reads `lp` before its clamp), and mode `M`'s
    /// output. Clamping `lp` first instead puts the RES 1 pitch 23 cents flat
    /// at 100 Hz falling to 40 at 5 kHz; this order holds −23 throughout.
    #[inline(always)]
    fn sub<const M: u8>(&mut self, x: f32, f: f32, q: f32) -> f32 {
        let lp = self.lp + f * self.bp;
        let hp = x - q * self.bp - lp;
        let bp = clampf(self.bp + f * hp, -DIRTY_STATE, DIRTY_STATE);
        let lp = clampf(lp, -DIRTY_STATE, DIRTY_STATE);
        (self.lp, self.bp) = (lp, bp);
        match DIRTY_MODES[M as usize] {
            FilterMode::Lp12 => lp,
            FilterMode::Bp12 => bp,
            // NOTCH1, a zero at FREQ: `x − q·bp`.
            FilterMode::Notch => lp + hp,
            // HP2; `DIRTY_MODES` holds no other.
            _ => hp,
        }
    }

    /// Forget the last `f`, `q` and LEVEL: the next block starts unramped,
    /// and LEVEL's clipper where its gate is.
    pub fn hold(&mut self) {
        self.last = None;
        self.level = LevelStage::default();
    }

    /// A fresh voice's filter: at rest and unramped.
    pub fn reset(&mut self) {
        (self.lp, self.bp) = (0.0, 0.0);
        self.hold();
    }

    /// Still ringing: a state above −80 dB at the output.
    pub fn rings(&self) -> bool {
        self.lp.abs().max(self.bp.abs()) * DIRTY_OUT > 1e-4
    }

    /// LEVEL's clipper runs next block (`LevelStage::running`).
    pub fn level_running(&self) -> bool {
        self.level.running()
    }

    /// `(lp, bp)`, for another type's warm start (spec § 1).
    pub fn outputs(&self) -> (f32, f32) {
        (self.lp, self.bp)
    }

    /// A warm start: `lp` and `bp` from another type's, held to the
    /// states' clamp.
    pub fn seed(&mut self, lp: f32, bp: f32) {
        let c = |v| clampf(v, -DIRTY_STATE, DIRTY_STATE);
        (self.lp, self.bp) = (c(lp), c(bp));
    }

    /// `[lp, bp]`, for the bound tests.
    #[cfg(feature = "test-support")]
    pub fn states(&self) -> [f32; 2] {
        [self.lp, self.bp]
    }

    /// The last block's `f` (`None`: no ramp next block).
    #[cfg(test)]
    pub(crate) fn last_f(&self) -> Option<f32> {
        self.last.map(|e| e.f)
    }
}
