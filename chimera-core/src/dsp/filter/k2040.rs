//! 2040: the SSM2040's four OTA stages (spec § 3, ADR 0096). Each stage is
//! a TPT one-pole whose OTA bends the slew on the stage's true error, its
//! output clamped to the chip's ±1 V swing. The global loop is solved
//! linearly in one step, then limited like a 3080. VOICE picks the drive
//! into the chip and, on EXP, a 3080 output stage: REV2 is the Prophet-5
//! Rev 1/2, EXP the RSF Kobol Expander. Same parameters; only constants
//! change. No compensation: the bass loss is the circuits'.

use super::coef::g_of;
use super::level::{FilterMods, LevelRamp, clip_amount, level_gain};
use crate::block::DiskCode;
use crate::dsp::clampf;
use crate::hw::{BLOCK_SIZE, Cost};
use crate::meter::Clip;
use crate::params::FilterParams;

/// REV2's drive into the chip, in model units (1 is the OTA's knee). Tuned
/// with `L_2040` (spec H-new-1): the 1 V/oct test on the limiter, then
/// test 7's full-scale bass loss and test 2's band on α. The Prophet-5's
/// stage-1 gain is 10 K/200 K (R480/R476).
pub const ALPHA_REV2: f32 = 0.1;
/// EXP's drive: 2 × REV2's, +6.0 dB. The Kobol's stage-1 gain is 10 K/100 K
/// (R25/R24) against the Prophet's 10 K/200 K. Never tuned on its own.
pub const ALPHA_EXP: f32 = 2.0 * ALPHA_REV2;
/// Every stage's output swing, both voices: "the output pins are only
/// capable of swinging ±1 V" (SSM 2040 data sheet, design hint 1). The
/// Prophet-5 shows "a 500-mV level" at each pole for a maximum saw
/// (tech manual § 5-13), read as peak, the grittier reading (peak to peak
/// would make it 4·α): 0.5 V is `ALPHA_REV2`, so 1 V is twice it.
pub const R_2040: f32 = 2.0 * ALPHA_REV2;
/// EXP's 3080 output VCA (the Kobol's CI3): R31 10 K into R40 220 Ω turns
/// ±1 V into ±21.5 mV, 0.416 × 2V_T, and the cubic matches `tanh` at 1.5 ×
/// that: `sat_2040(BETA_EXP·y₄)` reads 0.62 at the swing.
pub const BETA_EXP: f32 = 0.62 / R_2040;
pub const INV_BETA_EXP: f32 = R_2040 / 0.62;
/// The feedback limit, a 3080 in kind (spec B3): without it the OTAs
/// saturate inside the loop and the self-oscillation runs 330 cents flat.
/// Shared by both voices, so both pass one 1 V/oct test.
pub const L_2040: f32 = 0.25;
/// The output level: LEVEL ½ at RES 0 is 0 dB ± 0.5 on both voices.
pub const OUT_2040: f32 = 1.0;
/// The LED's knee, a fraction of the swing: stage 1 asked for more than
/// this lights it, `clip_amount` of the ratio (full at 3 × the knee). REV2
/// reaches it at LEVEL ≈ 0.75 on a full-scale input, EXP at ½.
pub const LED_KNEE: f32 = 0.9;

// The voices differ by the drive ratio alone, and the clip sits at REV2's
// 1 V; a model unit is the OTA's knee, so α stays well inside it.
const _: () = {
    assert!(ALPHA_REV2 > 0.0 && ALPHA_REV2 < 0.5);
    assert!(ALPHA_EXP == 2.0 * ALPHA_REV2 && R_2040 == ALPHA_EXP);
    assert!(BETA_EXP * R_2040 > 0.6199 && BETA_EXP * R_2040 < 0.6201);
    assert!(BETA_EXP * INV_BETA_EXP > 0.9999 && BETA_EXP * INV_BETA_EXP < 1.0001);
    assert!(L_2040 > 0.0 && OUT_2040 > 0.0);
    assert!(LED_KNEE > 0.0 && LED_KNEE <= 1.0);
};

/// Steady per-sample bills over CLEAN's LP24 (spec § Cost), pending Task
/// 18's recount: REV2's ≤ 78 instructions, EXP's POST on top.
const COST_REV2: u32 = 25;
const COST_EXP: u32 = 30;

/// The 2040's VOICE (Filter 13): which circuit's constants it runs. The
/// panel never names either instrument.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum K2040Voice {
    /// The Prophet-5 Rev 1/2: the chip run 6 dB under its swing, a linear
    /// SSM 2020 in the resonance.
    #[default]
    Rev2 = 0,
    /// The RSF Kobol Expander: 6 dB more drive, a 3080 in the resonance,
    /// another after the filter.
    Exp = 1,
}

/// VOICE's screen names, in `K2040Voice::ALL`'s order.
pub static VOICE_LABELS: [&str; 2] = ["REV2", "EXP"];

const _: () = {
    assert!(K2040Voice::ALL.len() == VOICE_LABELS.len());
    let mut i = 0;
    while i < K2040Voice::ALL.len() {
        assert!(K2040Voice::ALL[i] as usize == i);
        i += 1;
    }
};

impl K2040Voice {
    /// In discriminant order, as VOICE steps them.
    pub const ALL: [K2040Voice; 2] = [K2040Voice::Rev2, K2040Voice::Exp];

    pub const fn labels() -> &'static [&'static str; 2] {
        &VOICE_LABELS
    }

    /// The drive into the chip.
    pub const fn alpha(self) -> f32 {
        match self {
            K2040Voice::Rev2 => ALPHA_REV2,
            K2040Voice::Exp => ALPHA_EXP,
        }
    }

    /// `OUT_2040/α`, the output's scale, constant per voice: no divide in
    /// audio. EXP's also undoes POST's `β`.
    const fn out_scale(self) -> f32 {
        match self {
            K2040Voice::Rev2 => OUT_2040 / ALPHA_REV2,
            K2040Voice::Exp => OUT_2040 / ALPHA_EXP * INV_BETA_EXP,
        }
    }

    /// The 3080 output stage: EXP only.
    pub const fn post(self) -> bool {
        matches!(self, K2040Voice::Exp)
    }

    /// The steady bill over LP24, cycles a sample.
    pub const fn cost(self) -> Cost {
        Cost(match self {
            K2040Voice::Rev2 => COST_REV2,
            K2040Voice::Exp => COST_EXP,
        })
    }

    /// `v` as an index into `ALL`, held in range.
    pub fn from_index(v: f32) -> Self {
        Self::ALL[(v.max(0.0) as usize).min(Self::ALL.len() - 1)]
    }
}

impl DiskCode for K2040Voice {
    fn disk_code(self) -> u8 {
        match self {
            K2040Voice::Rev2 => 0,
            K2040Voice::Exp => 1,
        }
    }

    fn disk_ident(self) -> &'static str {
        match self {
            K2040Voice::Rev2 => "REV2",
            K2040Voice::Exp => "EXP",
        }
    }

    fn from_disk_code(c: u8) -> Option<Self> {
        match c {
            0 => Some(K2040Voice::Rev2),
            1 => Some(K2040Voice::Exp),
            _ => None,
        }
    }
}

/// RES's feedback `k`: 4 at RES 0.95, the onset every resonant type shares,
/// and 4.21 at RES 1.
#[inline(always)]
pub fn k2040_k(res: f32) -> f32 {
    res * (4.0 / 0.95)
}

/// The C1 cubic, `c − (4/27)·c³` for `c = clamp(v, ±1.5)`: slope 1 at 0,
/// flat at ±1 from ±1.5, no divide.
#[inline(always)]
pub fn sat_2040(v: f32) -> f32 {
    let c = clampf(v, -1.5, 1.5);
    c - (4.0 / 27.0) * c * c * c
}

/// The model's state: four stage integrators, the last block's ramps and
/// VOICE (`None` on a fresh filter, which starts unramped).
#[derive(Clone, Debug, Default)]
pub struct K2040 {
    s: [f32; 4],
    g: Option<f32>,
    k: Option<f32>,
    pre: Option<f32>,
    voice: Option<K2040Voice>,
}

/// One sample's coefficients at `g` and `k`: `G = g/(1 + g)`, `D = 1/(1 +
/// k·G⁴)` and the products the loop reads.
#[derive(Clone, Copy)]
struct Coef {
    g: f32,
    /// `g·4/27`: the stage's `g·sat(v) = v·(g − ga·v²)`.
    ga: f32,
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
        ga: g * (4.0 / 27.0),
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
    voice: K2040Voice,
}

fn ramp(last: &mut Option<f32>, to: f32) -> (f32, f32) {
    (last.replace(to).unwrap_or(to), to)
}

impl K2040 {
    pub fn new() -> Self {
        Self::default()
    }

    /// One block: the LED's clip, how far stage 1 asked past `LED_KNEE` of
    /// the swing.
    pub fn process(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        p: &FilterParams,
        fs: u32,
        mods: &FilterMods,
    ) -> Clip {
        let r = self.ramps(p, fs, mods);
        let (peak, sum) = match (r.voice.post(), r.g.0 == r.g.1 && r.k.0 == r.k.1) {
            (false, true) => self.steady::<false>(buf, r, |_, _| {}),
            (false, false) => self.moving::<false>(buf, r, |_, _| {}),
            (true, true) => self.steady::<true>(buf, r, |_, _| {}),
            (true, false) => self.moving::<true>(buf, r, |_, _| {}),
        };
        self.settle(sum);
        clip_amount(peak * (1.0 / (LED_KNEE * R_2040)))
    }

    /// `process`, with every sample's stage outputs `y₁..y₄` handed to
    /// `trace` (the stage-swing test).
    #[cfg(feature = "test-support")]
    pub fn process_traced(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        p: &FilterParams,
        fs: u32,
        trace: impl FnMut(usize, &[f32; 4]),
    ) {
        let r = self.ramps(p, fs, &FilterMods::NONE);
        let (_, sum) = match (r.voice.post(), r.g.0 == r.g.1 && r.k.0 == r.k.1) {
            (false, true) => self.steady::<false>(buf, r, trace),
            (false, false) => self.moving::<false>(buf, r, trace),
            (true, true) => self.steady::<true>(buf, r, trace),
            (true, false) => self.moving::<true>(buf, r, trace),
        };
        self.settle(sum);
    }

    /// A NaN or infinite input: the clamps hold every state finite, but at
    /// a rail, so start the next block from rest. `sum` is the block's
    /// inputs summed: not finite exactly when one was not (or the sum of
    /// finite ones overflowed, past any real signal).
    pub(super) fn settle(&mut self, sum: f32) {
        if !sum.is_finite() {
            self.s = [0.0; 4];
        }
    }

    /// This block's ramps, stored as the next block's start. A VOICE
    /// change rescales the states by `α_new/α_old`, exact for linear
    /// content since the output is `y/α` (spec § 3, never snapped).
    #[inline(always)]
    pub(super) fn ramps(&mut self, p: &FilterParams, fs: u32, mods: &FilterMods) -> Ramps {
        let v = p.voice;
        if let Some(old) = self.voice.replace(v)
            && old != v
        {
            let ratio = v.alpha() / old.alpha();
            for s in &mut self.s {
                *s *= ratio;
            }
        }
        let gain = LevelRamp::new(ramp(&mut self.pre, level_gain(p.level)), mods).gain();
        Ramps {
            g: ramp(&mut self.g, g_of(p.cutoff, fs as f32)),
            k: ramp(&mut self.k, k2040_k(p.resonance)),
            a: (gain.0 * v.alpha(), gain.1 * v.alpha()),
            voice: v,
        }
    }

    /// `g` and `k` held: `G` and `D` once a block. Returns stage 1's peak
    /// demand, `max|s₁ + t₁|`, and the inputs' sum (`settle`).
    #[inline(always)]
    pub(super) fn steady<const POST: bool>(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        mut trace: impl FnMut(usize, &[f32; 4]),
    ) -> (f32, f32) {
        let c = coef(r.g.1, r.k.1);
        let out = r.voice.out_scale();
        let (mut a, a_step) = (r.a.0, (r.a.1 - r.a.0) / BLOCK_SIZE as f32);
        let (mut peak, mut sum) = (0.0f32, 0.0f32);
        for (i, s) in buf.iter_mut().enumerate() {
            a += a_step;
            sum += *s;
            let (y, d1) = self.tick(*s * a, &c);
            trace(i, &y);
            peak = peak.max(d1.abs());
            *s = Self::output::<POST>(y[3], out);
        }
        (peak, sum)
    }

    /// `g` and `k` ramp to this block's values, reached on the last sample
    /// (#53) within an ulp or so, by adding as LEVEL's ramp does; `G` and
    /// `D` divide exactly every sample (B1).
    #[inline(always)]
    pub(super) fn moving<const POST: bool>(
        &mut self,
        buf: &mut [f32; BLOCK_SIZE],
        r: Ramps,
        mut trace: impl FnMut(usize, &[f32; 4]),
    ) -> (f32, f32) {
        let n = BLOCK_SIZE as f32;
        let (g_step, k_step) = ((r.g.1 - r.g.0) / n, (r.k.1 - r.k.0) / n);
        let out = r.voice.out_scale();
        let (mut a, a_step) = (r.a.0, (r.a.1 - r.a.0) / n);
        let (mut g, mut k) = (r.g.0, r.k.0);
        let (mut peak, mut sum) = (0.0f32, 0.0f32);
        for (i, s) in buf.iter_mut().enumerate() {
            (a, g, k) = (a + a_step, g + g_step, k + k_step);
            sum += *s;
            let c = coef(g, k);
            let (y, d1) = self.tick(*s * a, &c);
            trace(i, &y);
            peak = peak.max(d1.abs());
            *s = Self::output::<POST>(y[3], out);
        }
        (peak, sum)
    }

    /// `y₄` to the output: `y₄·OUT/α` on REV2; on EXP through the 3080,
    /// `sat(β·y₄)/β·OUT/α`, with `1/β` folded into `out`.
    #[inline(always)]
    fn output<const POST: bool>(y4: f32, out: f32) -> f32 {
        if POST {
            sat_2040(BETA_EXP * y4) * out
        } else {
            y4 * out
        }
    }

    /// One sample of the drive-scaled input `x_c`: the four stage outputs
    /// and stage 1's demand before its clamp.
    #[inline(always)]
    fn tick(&mut self, x_c: f32, c: &Coef) -> ([f32; 4], f32) {
        let [s1, s2, s3, s4] = self.s;
        // The stages' states through the linear cascade, then the global
        // loop solved linearly, then the 3080-style limit on its feedback.
        let big_s = c.one_g * (s4 + c.big_g * (s3 + c.big_g * (s2 + c.big_g * s1)));
        let u_l = x_c * c.d - c.kd * big_s;
        let u = x_c - L_2040 * sat_2040((x_c - u_l) * (1.0 / L_2040));
        let mut y = [0.0f32; 4];
        let mut x = u;
        let mut d1 = 0.0;
        for (i, s) in self.s.iter_mut().enumerate() {
            // The OTA on the true error, `g·sat(v) = v·(g − ga·v²)`.
            let v = clampf(c.one_g * (x - *s), -1.5, 1.5);
            let t = v * (c.g - c.ga * v * v);
            let d = *s + t;
            if i == 0 {
                d1 = d;
            }
            // The chip's ±1 V swing.
            let yi = clampf(d, -R_2040, R_2040);
            *s = yi + t;
            y[i] = yi;
            x = yi;
        }
        (y, d1)
    }

    /// Forget the last `g`, `k` and LEVEL: the next block starts unramped.
    pub fn hold(&mut self) {
        (self.g, self.k, self.pre) = (None, None, None);
    }

    /// A fresh voice's filter: at rest and unramped.
    pub fn reset(&mut self) {
        self.s = [0.0; 4];
        self.hold();
    }

    /// Still ringing: a state above −80 dB at the output.
    pub fn rings(&self) -> bool {
        let a = self.voice.unwrap_or_default().alpha();
        let m = self.s.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        m * (OUT_2040 / a) > 1e-4
    }

    /// The raw states, for the bound tests.
    #[cfg(feature = "test-support")]
    pub fn states(&self) -> [f32; 4] {
        self.s
    }

    /// The last block's `g` (`None`: no ramp next block).
    #[cfg(test)]
    pub(crate) fn last_g(&self) -> Option<f32> {
        self.g
    }
}
