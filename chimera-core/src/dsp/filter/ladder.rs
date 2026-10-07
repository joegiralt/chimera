//! LADDER: the Moog transistor ladder (ADR 0104). Four TPT one-poles, each
//! driven by the difference of its input's and its own output's saturator,
//! `ẏ = ω(tanh x − tanh y)` (Zavalishin § 6.9), and the loop fed back from
//! `y₄` through the input pair's saturator. The global loop is solved
//! linearly in one step, as on the 2040 (Zavalishin eq. 5.8); each stage
//! then reads its saturator at the linear estimate of its output, one
//! Newton step from zero (§ 6.6). The input pair and stage 1 saturate on a
//! Padé tanh, stages 2–4 on the 2040's divide-free cubic. 1× rate. No
//! compensation: the bass loss is the circuit's.
//!
//! MODEs mix the stage outputs `y₀ = u … y₄`, Xpander-style (§ 5.5).

use super::coef::g_of;
use super::k2040::{Ends, k2040_k, sat_2040};
use super::level::{FilterMods, LevelRamp, clip_amount, level_gain};
use super::{FilterMode, TapShape, ramps, tap_g_range};
use crate::dsp::clampf;
use crate::hw::{BLOCK_SIZE, Cost};
use crate::meter::Clip;
use crate::params::FilterParams;

/// The drive into the ladder at unity LEVEL, in model units (1 is the
/// input pair's knee, 2V_T): a full-scale input peaks at half the knee at
/// LEVEL ½, reaches it at LEVEL 0.75 (×2), where the LED lights, and runs
/// twice past it at LEVEL 1. It also sets the ring's level, the input
/// pair's, under the pass band: −15.6 dBFS (ADR 0104).
pub const ALPHA_LADDER: f32 = 0.5;
/// The output level: LEVEL ½ at RES 0 is 0 dB.
pub const OUT_LADDER: f32 = 1.0;
/// Stage 1's swing: where the Padé tanh flattens. Its equilibrium is the
/// tanh's inverse of the input pair's, so it never asks for more.
pub const Y1_MAX: f32 = 3.0;
/// Stages 2–4's swing: where `sat_2040` flattens, its inverse of ±1.
pub const Y_MAX: f32 = 1.5;
/// The LED's knee on the input pair, `|u|` in model units: 1 lights it
/// (tanh at 0.76), full at 3, where the Padé tanh clips.
pub const LED_KNEE: f32 = 1.0;
/// The cutoff trim, a ratio on FREQ, +15.8 cents: the stages' saturators
/// slow the ring at RES 1, which read 15.4 to 16.5 cents flat over C2–C6
/// untrimmed, as a Moog is trimmed at full resonance.
pub const TRIM_LADDER: f32 = 1.009169;

const _: () = {
    assert!(ALPHA_LADDER > 0.0 && ALPHA_LADDER < 1.0);
    assert!(OUT_LADDER > 0.0 && LED_KNEE > 0.0);
    assert!(Y1_MAX == 3.0 && Y_MAX == 1.5);
};

/// LADDER's modes, in knob order (CLEAN's order): LP4 the default.
pub(crate) const LADDER_MODES: [FilterMode; 5] = [
    FilterMode::Lp12,
    FilterMode::Lp24,
    FilterMode::Bp12,
    FilterMode::Bp24,
    FilterMode::Hp24,
];
pub(crate) const LADDER_LABELS: [&str; 5] = ["LP2", "LP4", "BP2", "BP4", "HP4"];

/// A mode's mix of `y₀ … y₄` and its gain at `k = 0` and from the onset
/// (`k ≥ 4`), lerped in `k` between.
#[derive(Clone, Copy)]
pub(crate) struct Tap {
    pub(crate) c: [f32; 5],
    gain: [f32; 2],
}

/// `LADDER_MODES`' mixes, by Zavalishin's `s → s − 1` rule on `H_n(s) =
/// (1 + s)^(4−n)/((1 + s)⁴ + k)` (§ 5.5): LP2 `(1+s)²`, LP4 `1`, BP2
/// `s(1+s)²`, BP4 `s²`, HP4 `s⁴` over the shared denominator (derived, not
/// from a source: `modes_respond_as_derived` checks each). The gains bring
/// each to 0 dB at RES 0 (BP2 peaks at ½, BP4 at ¼) and each ring to LP4's
/// level (at the cutoff LP2's and BP2's numerators are 2, the rest 1).
pub(crate) const LADDER_TAPS: [Tap; 5] = [
    Tap {
        c: [0.0, 0.0, 1.0, 0.0, 0.0],
        gain: [1.0, 0.5],
    },
    Tap {
        c: [0.0, 0.0, 0.0, 0.0, 1.0],
        gain: [1.0, 1.0],
    },
    Tap {
        c: [0.0, 1.0, -1.0, 0.0, 0.0],
        gain: [2.0, 0.5],
    },
    Tap {
        c: [0.0, 0.0, 1.0, -2.0, 1.0],
        gain: [4.0, 1.0],
    },
    Tap {
        c: [1.0, -4.0, 6.0, -4.0, 1.0],
        gain: [1.0, 1.0],
    },
];

const _: () = {
    assert!(LADDER_MODES.len() == LADDER_TAPS.len());
    assert!(LADDER_MODES.len() == LADDER_LABELS.len());
};

/// `mode`'s index in `LADDER_MODES`. `FilterParams::set_mode` keeps a
/// LADDER Sound's mode in the list; anything else plays LP4.
pub(crate) fn ladder_index(mode: FilterMode) -> usize {
    match mode {
        FilterMode::Lp12 => 0,
        FilterMode::Bp12 => 2,
        FilterMode::Bp24 => 3,
        FilterMode::Hp24 => 4,
        FilterMode::Lp24
        | FilterMode::Lp6
        | FilterMode::Notch
        | FilterMode::Phaser
        | FilterMode::Hp12 => 1,
    }
}

/// Mode `M`'s gain at `k`: its RES-0 gain, lerped to its ring's at the
/// onset.
#[inline(always)]
fn tap_gain(m: usize, k: f32) -> f32 {
    let [g0, g4] = LADDER_TAPS[m].gain;
    g0 + (g4 - g0) * clampf(k * 0.25, 0.0, 1.0)
}

/// LADDER's steady bill over `OLD_LP24` (95): LP4 counted at 81
/// instructions a sample and 2 `vdiv` (the Padé tanhs), ceil5(81 × 1.606
/// + 24) = 155, less 95: 60. HP4's mix adds 4: 165, 70. Billed the larger.
pub const LADDER_COST: Cost = Cost(70);
/// The moving path: `G` and `D` divided every sample (2 `vdiv`) and the
/// ramps' fill, over the steady one.
pub const LADDER_MOVE_COST: Cost = Cost(60);

/// The Padé 3/2 tanh, `x(27 + x²)/(27 + 9x²)`, held at ±1 from ±3 where it
/// peaks: within 0.024 of tanh, one divide.
#[inline(always)]
pub fn pade_tanh(x: f32) -> f32 {
    let c = clampf(x, -3.0, 3.0);
    let c2 = c * c;
    c * (27.0 + c2) / (27.0 + 9.0 * c2)
}

/// The output's bound on mode `mode` for inputs within `±x` at LEVEL 1
/// (×4): each stage within its swing, every state within it plus `2g` at
/// the top `g`, `u` within `|x_c| + k·S` (`S` mixes the states with
/// weights summing under 1), each mix term at its bound, at the mode's
/// larger gain. Loose for the band and high modes; LP4's is `Y_MAX/α`.
#[cfg(feature = "test-support")]
pub fn ladder_bound(mode: FilterMode, x: f32) -> f32 {
    let g = super::coef::g_of(f32::MAX, 1.0);
    let s = Y1_MAX + 2.0 * 2.0 * g;
    let u = 4.0 * ALPHA_LADDER * x + k2040_k(1.0) * s;
    let y = [u, Y1_MAX, Y_MAX, Y_MAX, Y_MAX];
    let t = LADDER_TAPS[ladder_index(mode)];
    let m: f32 = t.c.iter().zip(y).map(|(c, y)| c.abs() * y).sum();
    m * t.gain[0].max(t.gain[1]) * (OUT_LADDER / ALPHA_LADDER) * 1.0001
}

/// The model's state: four stage integrators and the last block's ramp
/// ends (`None` on a fresh filter, which starts unramped).
#[derive(Clone, Debug, Default)]
pub struct Ladder {
    s: [f32; 4],
    last: Option<Ends>,
}

/// One sample's coefficients at `g` and `k`: `G = g/(1 + g)`, `D = 1/(1 +
/// k·G⁴)`.
#[derive(Clone, Copy)]
struct Coef {
    g: f32,
    big_g: f32,
    one_g: f32,
    d: f32,
    kd: f32,
}

#[inline(always)]
fn coef(g: f32, k: f32) -> Coef {
    let big_g = g / (1.0 + g);
    let g2 = big_g * big_g;
    let d = 1.0 / (1.0 + k * g2 * g2);
    Coef {
        g,
        big_g,
        one_g: 1.0 - big_g,
        d,
        kd: k * d,
    }
}

/// One block's `(from, to)` for `g`, `k` and the input scale `α·gain`.
#[derive(Clone, Copy)]
pub(super) struct Ramps {
    g: (f32, f32),
    k: (f32, f32),
    a: (f32, f32),
}

impl Ladder {
    pub fn new() -> Self {
        Self::default()
    }

    /// One block: the LED's clip, how far the input pair was asked past
    /// `LED_KNEE`.
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        p: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> Clip {
        let r = self.ramps(p, fs, mods);
        let t = TapShape::new(mods, tap_g_range(fs), k2040_k);
        let t = t.as_ref();
        let (peak, sum) = match ladder_index(p.mode()) {
            0 => self.run::<0>(buf, r, t),
            2 => self.run::<2>(buf, r, t),
            3 => self.run::<3>(buf, r, t),
            4 => self.run::<4>(buf, r, t),
            _ => self.run::<1>(buf, r, t),
        };
        self.settle(sum);
        clip_amount(peak * (1.0 / LED_KNEE))
    }

    /// Mode `M`'s block, steady or moving; a FREQ or Q tap moves it.
    #[inline(always)]
    fn run<const M: usize>(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        tap: Option<&TapShape>,
    ) -> (f32, f32) {
        if tap.is_none() && r.g.0 == r.g.1 && r.k.0 == r.k.1 {
            self.steady::<M>(buf, r)
        } else {
            self.moving::<M>(buf, r, tap)
        }
    }

    /// A NaN or infinite input: the clamps hold every state finite, but at
    /// a rail, so start the next block from rest. `sum` is the block's
    /// inputs summed.
    pub(super) fn settle(&mut self, sum: f32) {
        if !sum.is_finite() {
            self.s = [0.0; 4];
        }
    }

    /// A warm start from another type's `lp` (spec § 1): every stage at its
    /// DC steady state for that input, `lp/(1 + k)` (the loop's bass loss),
    /// in model units, within each stage's swing.
    pub fn seed(&mut self, lp: f32, res: f32) {
        let s = lp * (ALPHA_LADDER / OUT_LADDER) / (1.0 + k2040_k(res));
        let c = clampf(s, -Y_MAX, Y_MAX);
        self.s = [clampf(s, -Y1_MAX, Y1_MAX), c, c, c];
    }

    /// `y₄` as another type's `lp` (spec § 1): at the output's scale and the
    /// bass loss undone, `(1 + k)·y₄/α`.
    pub fn y4(&self, res: f32) -> f32 {
        self.s[3] * (OUT_LADDER / ALPHA_LADDER) * (1.0 + k2040_k(res))
    }

    /// The stages and the ramp ends, which a fade to or from the 2040
    /// hands across (`Filter`'s one four-pole slot, ADR 0101).
    pub(super) fn parts(&self) -> ([f32; 4], Option<Ends>) {
        (self.s, self.last)
    }

    pub(super) fn from_parts(s: [f32; 4], last: Option<Ends>) -> Self {
        Self { s, last }
    }

    /// This block's ramps, stored as the next block's start.
    #[inline(always)]
    pub(super) fn ramps(&mut self, p: &FilterParams, fs: u32, mods: &FilterMods) -> Ramps {
        let to = Ends {
            g: g_of(p.cutoff * TRIM_LADDER, fs as f32),
            k: k2040_k(p.resonance),
            pre: level_gain(p.level),
        };
        let from = self.last.replace(to).unwrap_or(to);
        let gain = LevelRamp::new((from.pre, to.pre), mods).gain();
        Ramps {
            g: (from.g, to.g),
            k: (from.k, to.k),
            a: (gain.0 * ALPHA_LADDER, gain.1 * ALPHA_LADDER),
        }
    }

    /// `g` and `k` held: `G` and `D` once a block. Returns the input pair's
    /// peak `|u|` and the inputs' sum (`settle`).
    #[inline(always)]
    pub(super) fn steady<const M: usize>(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
    ) -> (f32, f32) {
        let c = coef(r.g.1, r.k.1);
        let n = BLOCK_SIZE as f32;
        let (mut a, a_step) = (r.a.0, (r.a.1 - r.a.0) / n);
        let out = OUT_LADDER / ALPHA_LADDER;
        let (mut o, o_step) = Self::gains::<M>(r, out);
        let (mut peak, mut sum) = (0.0f32, 0.0f32);
        for s in buf.iter_mut() {
            a += a_step;
            o += o_step;
            sum += *s;
            let y = self.tick(*s * a, &c);
            peak = peak.max(y[0].abs());
            *s = Self::mix::<M>(&y) * o;
        }
        (peak, sum)
    }

    /// `g` and `k` ramp to this block's values by adding, each sample's
    /// through the taps if any (`TapShape`); `G` and `D` divide exactly
    /// every sample (B1). The mode's gain follows the ramp's `k`.
    #[inline(always)]
    pub(super) fn moving<const M: usize>(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        tap: Option<&TapShape>,
    ) -> (f32, f32) {
        let n = BLOCK_SIZE as f32;
        let (g_step, k_step) = ((r.g.1 - r.g.0) / n, (r.k.1 - r.k.0) / n);
        let (mut gs, mut ks) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
        ramps(&mut gs, &mut ks, (r.g.0, g_step), (r.k.0, k_step), true);
        if let Some(t) = tap {
            t.apply(&mut gs, &mut ks);
        }
        let (mut a, a_step) = (r.a.0, (r.a.1 - r.a.0) / n);
        let out = OUT_LADDER / ALPHA_LADDER;
        let (mut o, o_step) = Self::gains::<M>(r, out);
        let (mut peak, mut sum) = (0.0f32, 0.0f32);
        for (i, s) in buf.iter_mut().enumerate() {
            a += a_step;
            o += o_step;
            sum += *s;
            let c = coef(gs[i], ks[i]);
            let y = self.tick(*s * a, &c);
            peak = peak.max(y[0].abs());
            *s = Self::mix::<M>(&y) * o;
        }
        (peak, sum)
    }

    /// The output gain's start and step: the mode's gain at each end's `k`
    /// times `out`, ramped across the block.
    #[inline(always)]
    fn gains<const M: usize>(r: Ramps, out: f32) -> (f32, f32) {
        let (g0, g1) = (tap_gain(M, r.k.0) * out, tap_gain(M, r.k.1) * out);
        (g0, (g1 - g0) / BLOCK_SIZE as f32)
    }

    /// Mode `M`'s mix of `y₀ … y₄`; the zero terms fold away.
    #[inline(always)]
    fn mix<const M: usize>(y: &[f32; 5]) -> f32 {
        let c = LADDER_TAPS[M].c;
        let mut m = 0.0;
        for i in 0..5 {
            if c[i] != 0.0 {
                m += c[i] * y[i];
            }
        }
        m
    }

    /// One sample of the drive-scaled input `x_c`: `y₀ = u … y₄`.
    #[inline(always)]
    fn tick(&mut self, x_c: f32, c: &Coef) -> [f32; 5] {
        let [s1, s2, s3, s4] = self.s;
        // The stages' states through the linear cascade, then the global
        // loop solved linearly (Zavalishin eq. 5.8).
        let big_s = c.one_g * (s4 + c.big_g * (s3 + c.big_g * (s2 + c.big_g * s1)));
        let u = x_c * c.d - c.kd * big_s;
        // The input pair, then each stage on `sat(in) − sat(y)`, `sat(y)` read
        // at the stage's linear estimate and handed on as the next input.
        let mut w = pade_tanh(u);
        let mut y = [u, 0.0, 0.0, 0.0, 0.0];
        for (i, s) in self.s.iter_mut().enumerate() {
            let est = c.big_g * w + c.one_g * *s;
            let (sat, max) = if i == 0 {
                (pade_tanh(est), Y1_MAX)
            } else {
                (sat_2040(est), Y_MAX)
            };
            let t = c.g * (w - sat);
            let yi = clampf(*s + t, -max, max);
            *s = yi + t;
            y[i + 1] = yi;
            w = sat;
        }
        y
    }

    /// One sample of the raw model at `g` and `k`, `x_c` in model units:
    /// `y₀ … y₄`. The A/B harness's entry (ADR 0104); not the audio path.
    #[cfg(feature = "test-support")]
    pub fn tick_raw(&mut self, x_c: f32, g: f32, k: f32) -> [f32; 5] {
        self.tick(x_c, &coef(g, k))
    }

    /// Forget the last `g`, `k` and LEVEL: the next block starts unramped.
    pub fn hold(&mut self) {
        self.last = None;
    }

    /// A fresh voice's filter: at rest and unramped.
    pub fn reset(&mut self) {
        self.s = [0.0; 4];
        self.hold();
    }

    /// Still ringing: a state above −80 dB at the output.
    pub fn rings(&self) -> bool {
        let m = self.s.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        m * (OUT_LADDER / ALPHA_LADDER) > 1e-4
    }

    /// The raw states, for the bound tests.
    #[cfg(feature = "test-support")]
    pub fn states(&self) -> [f32; 4] {
        self.s
    }
}
