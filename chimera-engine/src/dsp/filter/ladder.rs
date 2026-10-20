//! LADDER: the Moog transistor ladder (ADR 0104). Four TPT one-poles, each
//! driven by the difference of its input's and its own output's saturator,
//! `ẏ = ω(tanh x − tanh y)` (Zavalishin § 6.9), and the loop fed back from
//! `y₄` through the input pair's saturator. The global loop is solved
//! linearly in one step, as on the 2040 (Zavalishin eq. 5.8); each stage
//! then reads its saturator at the linear estimate of its output, one
//! Newton step from zero (§ 6.6). All five saturate on one tanh, a
//! divide-free odd polynomial (`tanh_poly`): measured against SPICE it
//! beats the Padé 3/2 tanh the design began with, and the 2040's cubic on
//! stages 2–4 put the large-signal cutoff 47 cents off (ADR 0104). 1× rate.
//! No compensation: the bass loss is the circuit's.
//!
//! MODEs mix the stage outputs `y₀ = u … y₄`, Xpander-style (§ 5.5).

use super::k2040::{Ends, four_pole_ends, k2040_k};
use super::level::{FilterMods, clip_amount};
use super::{Fed, FilterMode, SUB, TapShape, ramps, step_to, tap_g_range};
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
/// Every stage's swing: where `tanh_poly` flattens. A stage's
/// equilibrium is the tanh's inverse of its input's, so it never asks for
/// more.
pub const Y_MAX: f32 = 3.0;
/// The input's hold, in input units: 32 × full scale. A block with a
/// sample past it is held to it before the loop (`process`); none reaches
/// it from the engines.
pub const X_MAX: f32 = 32.0;
/// The LED's knee on the drive into the input pair, `|x|·α·gain` in model
/// units: 1 lights it (tanh at 0.76), full at 3, where `tanh_poly`
/// flattens.
pub const LED_KNEE: f32 = 1.0;
/// The cutoff trim, a ratio on FREQ, +20.8 cents: the stages' saturators
/// slow the ring at RES 1, which reads 20.2 to 21.4 cents flat over C2–C6
/// untrimmed (SPICE's BJT ladder rings 19 flat at k = 4.3), and a Moog is
/// trimmed at full resonance.
pub const TRIM_LADDER: f32 = 1.012088;

const _: () = {
    assert!(ALPHA_LADDER > 0.0 && ALPHA_LADDER < 1.0);
    assert!(OUT_LADDER > 0.0 && LED_KNEE > 0.0);
    assert!(Y_MAX == 3.0);
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
/// `s(1+s)²`, BP4 `s²`, HP4 `s⁴` over the shared denominator (BP4 is his
/// eq. 5.16, the rest derived by the rule; `modes_respond_as_derived`
/// checks each). The gains bring
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

/// `mode`'s output gain at `k` (the viz draws it).
pub fn mode_gain(mode: FilterMode, k: f32) -> f32 {
    tap_gain(ladder_index(mode), k)
}

/// Mode `M`'s gain at `k`: its RES-0 gain, lerped to its ring's at the
/// onset.
#[inline(always)]
fn tap_gain(m: usize, k: f32) -> f32 {
    let [g0, g4] = LADDER_TAPS[m].gain;
    g0 + (g4 - g0) * clampf(k * 0.25, 0.0, 1.0)
}

/// LADDER's steady bill over `OLD_LP24` (95) as the unit reads it
/// (2026-10-19, rev V, ROUTING bench, run 2, `/VOICE` slope): 176 (#642).
/// History: the host count was 115, from the `filter-probes` ELF
/// (`tools/loop_count.py`): one loop for every mode,
/// its mix read at run time (`Ladder::steady`), 119 instructions a sample,
/// 0 `vdiv`; its block (1.00, 2 `vdiv`), the probe's (0.66),
/// `four_pole_ends` (1.33, 1 `vdiv`) and `hold_huge`'s scan (29 a
/// four-sample round, 7.25, and its block 0.3): ceil5(129.54 × 1.606 + 12
/// × 0.047) = 210, less 95. Per-mode loops cost 2.8 KB the
/// `bench-stress,orbit` build doesn't have.
pub const LADDER_COST: Cost = Cost(176);
/// The moving path over the steady one: one out-of-line loop for every
/// mode (`moving`, 140 instructions), `G` and `D` divided every sample (2
/// `vdiv`), the mode gain at each sample's `k`, its block (1.34), the
/// probe's (0.69), `four_pole_ends`, the scan and `ramps`' fill (6.25):
/// 157.16 instructions, 2.016 `vdiv`, 280; less the steady 210.
pub const LADDER_MOVE_COST: Cost = Cost(70);

/// tanh to within 0.005 on ±3, flat at ±1 from ±3 where it is held: the
/// odd degree-11 polynomial with `p(3) = 1`, `p′(3) = 0` that fits tanh
/// best on [0, 3] (least worst error, kept monotone; fitted for ADR 0104,
/// not from a source). No divide. Against tanh its third harmonic is
/// tanh's, unlike `sat_2040`'s 4/27 cubic. Summed by powers, `cⁿ` times
/// its coefficient into one accumulator: each term a multiply and a
/// multiply-add, where Horner's form spends a copy of its constant as
/// well (the M7's `vmla` accumulates into its destination).
#[inline(always)]
#[allow(clippy::excessive_precision)] // the fit's digits
pub fn tanh_poly(x: f32) -> f32 {
    let c = clampf(x, -3.0, 3.0);
    let c2 = c * c;
    let mut t = c * c2;
    let mut p = c - 0.30530142 * t;
    t *= c2;
    p += 0.0812583861 * t;
    t *= c2;
    p -= 0.0130879632 * t;
    t *= c2;
    p += 0.00109139566 * t;
    t *= c2;
    p - 3.5909244e-05 * t
}

/// The output's bound on mode `mode` for inputs within `±x` at LEVEL 1
/// (×4): each stage within its swing, every state within it plus `2g` at
/// the top `g`, `u` within `|x_c| + k·S` (`S` mixes the states with
/// weights summing under 1), each mix term at its bound, at the mode's
/// larger gain. Loose for the band and high modes; LP4's is `Y_MAX/α`. A
/// stability bound, valid at every `k`, so it can't see a mode's level:
/// BP4's ×4 at RES 0 must be in it. `a_q_tap_rings_every_mode_at_lp4s_level`
/// pins the levels (review I1).
#[cfg(feature = "test-support")]
pub fn ladder_bound(mode: FilterMode, x: f32) -> f32 {
    let g = super::coef::g_of(f32::MAX, 1.0);
    let s = Y_MAX + 2.0 * 2.0 * g;
    let u = 4.0 * ALPHA_LADDER * x.min(X_MAX) + k2040_k(1.0) * s;
    let y = [u, Y_MAX, Y_MAX, Y_MAX, Y_MAX];
    let t = LADDER_TAPS[ladder_index(mode)];
    let m: f32 = t.c.iter().zip(y).map(|(c, y)| c.abs() * y).sum();
    m * t.gain[0].max(t.gain[1]) * (OUT_LADDER / ALPHA_LADDER) * 1.0001
}

/// A block held to ±`X_MAX` before the loop, so no mode's output can
/// overflow: an infinite or huge sample is held, and the loop then runs on
/// finite input: the block's peak `|x|`, held. `None` for a block with a
/// NaN in it, which `process` plays as silence. The scan reads `|x|`'s bits, an `and` and an unsigned
/// max a sample (a NaN's bits sort past infinity's); the hold and the NaN
/// search run only for a block past `X_MAX`.
#[inline(never)]
pub(super) fn hold_huge(buf: &mut [f32; BLOCK_SIZE]) -> Option<f32> {
    // Four samples a round, the rounds kept rolled (an opaque index): a
    // sixteen-fold unroll cost 200 B the ORBIT bench build hasn't got.
    let mut m = 0u32;
    let mut i = 0;
    while i < BLOCK_SIZE {
        let j = core::hint::black_box(i);
        if let Some(q) = buf.get(j..j + 4) {
            for x in q {
                m = m.max(x.to_bits() & 0x7fff_ffff);
            }
        }
        i += 4;
    }
    (m <= X_MAX.to_bits() || hold_slow(buf)).then(|| f32::from_bits(m).min(X_MAX))
}

/// `hold_huge`'s rare half: each sample held to ±`X_MAX`, `false` at a NaN.
/// One loop, kept rolled (an opaque index), so it costs little flash.
#[cold]
#[inline(never)]
fn hold_slow(buf: &mut [f32; BLOCK_SIZE]) -> bool {
    let mut i = 0;
    while i < BLOCK_SIZE {
        if let Some(x) = buf.get_mut(core::hint::black_box(i)) {
            if x.is_nan() {
                return false;
            }
            *x = clampf(*x, -X_MAX, X_MAX);
        }
        i += 1;
    }
    true
}

/// The model's state: four stage integrators and the last block's ramp
/// ends (`None` on a fresh filter, which starts unramped).
#[derive(Clone, Debug, Default)]
pub struct Ladder {
    s: [f32; 4],
    last: Option<Ends>,
    /// FB's signal (#393).
    fbp: f32,
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
        let Some(x_peak) = hold_huge(buf) else {
            self.silence(buf);
            return clip_amount(0.0);
        };
        // The LED: the drive into the input pair, the block's peak at its
        // larger LEVEL end, from the scan; no compare in the loops.
        let drive = x_peak * r.a.0.max(r.a.1);
        let m = ladder_index(p.mode());
        let still = t.is_none() && !mods.fb.on();
        if still && r.g.0 == r.g.1 && r.k.0 == r.k.1 {
            self.steady(buf, r, m);
        } else if still && mods.glide {
            self.stepped(buf, r, m);
        } else {
            // FB's loop is bank 2's (ADR 0179); `moving` runs as it was.
            if !(mods.fb.on() && self.fed_play(buf, r, t, m, mods.fb, tap_g_range(fs))) {
                self.moving(buf, r, t, m);
            }
        }
        if !mods.fb.on() {
            self.fbp = 0.0;
        }
        clip_amount(drive * (1.0 / LED_KNEE))
    }

    /// A block with a NaN in it (`hold_huge`): silence, since HP4's `y₀ = u`
    /// would pass it on, and the next block from rest.
    pub(super) fn silence(&mut self, buf: &mut [f32; BLOCK_SIZE]) {
        buf.fill(0.0);
        self.s = [0.0; 4];
    }

    /// A warm start from another type's `lp` (spec § 1): every stage at its
    /// DC steady state for that input, `lp/(1 + k)` (the loop's bass loss),
    /// in model units, within each stage's swing.
    pub fn seed(&mut self, lp: f32, res: f32) {
        let s = lp * (ALPHA_LADDER / OUT_LADDER) / (1.0 + k2040_k(res));
        self.s = [clampf(s, -Y_MAX, Y_MAX); 4];
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
        Self { s, last, fbp: 0.0 }
    }

    /// This block's ramps, stored as the next block's start.
    #[inline(always)]
    pub(super) fn ramps(&mut self, p: &FilterParams, fs: u32, mods: &FilterMods) -> Ramps {
        let (from, to, gain) = four_pole_ends(&mut self.last, p, TRIM_LADDER, fs, mods);
        Ramps {
            g: mods.spread.g((from.g, to.g)),
            k: mods.spread.k((from.k, to.k)),
            a: (gain.0 * ALPHA_LADDER, gain.1 * ALPHA_LADDER),
        }
    }

    /// `g` and `k` held: `G` and `D` once a block. One loop for every mode,
    /// `m` read at run time, as `moving`'s: five copies didn't fit the
    /// ORBIT bench builds.
    #[inline(never)]
    pub(super) fn steady(&mut self, buf: &mut [f32; BLOCK_SIZE], r: Ramps, m: usize) {
        let c = coef(r.g.1, r.k.1);
        let n = BLOCK_SIZE as f32;
        let (mut a, a_step) = (r.a.0, (r.a.1 - r.a.0) / n);
        let out = OUT_LADDER / ALPHA_LADDER;
        // `k` is held, so the mode's gain is too: folded into its mix.
        let m = m.min(LADDER_TAPS.len() - 1);
        let o = tap_gain(m, r.k.1) * out;
        let c5 = LADDER_TAPS[m].c.map(|c| c * o);
        for s in buf.iter_mut() {
            a += a_step;
            let y = self.tick(*s * a, &c);
            *s = Self::mix(&c5, &y);
        }
    }

    /// A knob glide: `g` and `k` step `STEPS` times to this block's values,
    /// each sub-block the steady loop (ADR 0117).
    #[inline(never)]
    pub(super) fn stepped(&mut self, buf: &mut [f32; BLOCK_SIZE], r: Ramps, m: usize) {
        let (mut a, a_step) = (r.a.0, (r.a.1 - r.a.0) / BLOCK_SIZE as f32);
        let out = OUT_LADDER / ALPHA_LADDER;
        let m = m.min(LADDER_TAPS.len() - 1);
        for (j, sub) in buf.as_chunks_mut::<SUB>().0.iter_mut().enumerate() {
            let (g, k) = (step_to(r.g, j), step_to(r.k, j));
            let c = coef(g, k);
            let o = tap_gain(m, k) * out;
            let c5 = LADDER_TAPS[m].c.map(|c| c * o);
            for s in sub {
                a += a_step;
                let y = self.tick(*s * a, &c);
                *s = Self::mix(&c5, &y);
            }
        }
    }

    /// `g` and `k` ramp to this block's values by adding, each sample's
    /// through the taps if any (`TapShape`); `G` and `D` divide exactly
    /// every sample (B1). The mode's gain follows each sample's `k`, so a Q
    /// tap rings every mode at LP4's level (review I1): one clamp and two
    /// multiply-adds, no divide. One loop for every mode, `m`
    /// (`LADDER_MODES`' index) read at run time: the divides dwarf the mix's
    /// three spare multiply-adds, and four fewer copies keep the bench
    /// builds in FLASH.
    #[inline(never)]
    pub(super) fn moving(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        tap: Option<&TapShape>,
        m: usize,
    ) {
        let n = BLOCK_SIZE as f32;
        let (g_step, k_step) = ((r.g.1 - r.g.0) / n, (r.k.1 - r.k.0) / n);
        let (mut gs, mut ks) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
        ramps(&mut gs, &mut ks, (r.g.0, g_step), (r.k.0, k_step), true);
        if let Some(t) = tap {
            t.apply(&mut gs, &mut ks);
        }
        let (mut a, a_step) = (r.a.0, (r.a.1 - r.a.0) / n);
        let out = OUT_LADDER / ALPHA_LADDER;
        let m = m.min(LADDER_TAPS.len() - 1);
        let Tap {
            c: c5,
            gain: [g0, g4],
        } = LADDER_TAPS[m];
        let (o0, o4) = (g0 * out, (g4 - g0) * out);
        for (i, s) in buf.iter_mut().enumerate() {
            a += a_step;
            let o = o0 + o4 * clampf(ks[i] * 0.25, 0.0, 1.0);
            let c = coef(gs[i], ks[i]);
            let y = self.tick(*s * a, &c);
            *s = Self::mix(&c5, &y) * o;
        }
    }

    /// FB's block (ADR 0179): `moving`'s arrays and mix, built here, and
    /// the loop in bank 2 (`fed::Ladder::fed`). False while the door is
    /// closed.
    fn fed_play(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        tap: Option<&TapShape>,
        m: usize,
        fb: Fed,
        gr: (f32, f32),
    ) -> bool {
        let n = BLOCK_SIZE as f32;
        let (g_step, k_step) = ((r.g.1 - r.g.0) / n, (r.k.1 - r.k.0) / n);
        let (mut gs, mut ks) = ([0.0f32; BLOCK_SIZE], [0.0f32; BLOCK_SIZE]);
        ramps(&mut gs, &mut ks, (r.g.0, g_step), (r.k.0, k_step), true);
        if let Some(t) = tap {
            t.apply(&mut gs, &mut ks);
        }
        let out = OUT_LADDER / ALPHA_LADDER;
        let Tap {
            c: c5,
            gain: [g0, g4],
        } = LADDER_TAPS[m.min(LADDER_TAPS.len() - 1)];
        let mix = FedMix {
            a: (r.a.0, (r.a.1 - r.a.0) / n),
            c5,
            o: (g0 * out, (g4 - g0) * out),
        };
        super::fed::through(|d| (d.ladder)(self, buf, &gs, &ks, mix, fb, gr)).is_some()
    }

    /// A mode's mix `c` of `y₀ … y₄`, five multiply-adds.
    #[inline(always)]
    fn mix(c: &[f32; 5], y: &[f32; 5]) -> f32 {
        c[0] * y[0] + c[1] * y[1] + c[2] * y[2] + c[3] * y[3] + c[4] * y[4]
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
        let mut w = tanh_poly(u);
        let mut y = [u, 0.0, 0.0, 0.0, 0.0];
        for (i, s) in self.s.iter_mut().enumerate() {
            let sat = tanh_poly(c.big_g * w + c.one_g * *s);
            let t = c.g * (w - sat);
            let yi = clampf(*s + t, -Y_MAX, Y_MAX);
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
        (self.s, self.fbp) = ([0.0; 4], 0.0);
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

/// What FB's loop reads besides `g` and `k`: the input scale's `(from,
/// step)`, the mode's mix and its gain's `(at k 0, slope a k/4)`.
#[derive(Clone, Copy)]
pub struct FedMix {
    a: (f32, f32),
    c5: [f32; 5],
    o: (f32, f32),
}

/// FB's loop (ADR 0179), linked into bank 2 with `bank2` (`filter::fed`).
mod fed {
    use super::super::Fed;
    use super::{FedMix, Ladder, coef};
    use crate::dsp::clampf;
    use crate::hw::BLOCK_SIZE;

    impl Ladder {
        /// `moving`'s loop, each sample's `g` through the FB signal.
        #[inline(never)]
        pub(in crate::dsp::filter) fn fed(
            &mut self,
            buf: &mut [f32; BLOCK_SIZE],
            gs: &[f32; BLOCK_SIZE],
            ks: &[f32; BLOCK_SIZE],
            m: FedMix,
            fb: Fed,
            gr: (f32, f32),
        ) {
            let (mut a, a_step) = m.a;
            for (i, s) in buf.iter_mut().enumerate() {
                a += a_step;
                let o = m.o.0 + m.o.1 * clampf(ks[i] * 0.25, 0.0, 1.0);
                let c = coef(fb.g(gs[i], i, self.fbp, gr), ks[i]);
                let y = self.tick(*s * a, &c);
                *s = Self::mix(&m.c5, &y) * o;
                fb.feed(&mut self.fbp, *s);
            }
        }
    }
}
