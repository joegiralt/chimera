//! GRIT as the reverb's converters (reverb TYPEs spec § GRIT): an ADC on
//! the decimated send and a DAC on the return, outside every engine. Each
//! is a one-pole high cut, TPDF dither and a rounding quantiser of
//! `bits(grit)` bits over ±1.0. The loop never quantises by GRIT.

use crate::dsp::algo::math::exp2;
use crate::dsp::clampf;
use crate::dsp::halfband::HALF;
use crate::dsp::ring::rint;
use crate::dsp::xorshift32;

use super::parts::sin_turn;

/// Ring samples a converter's dither holds after its last non-zero input
/// (100 ms at 24 kHz), then fades over (50 ms).
pub const IDLE_HOLD: u32 = 2_400;
pub const IDLE_FADE: u32 = 1_200;
/// Ring samples a woken converter's dither takes back up to full.
const WAKE: u32 = 24;
const WAKE_STEP: u32 = IDLE_FADE / WAKE;
const _: () = assert!(IDLE_FADE.is_multiple_of(WAKE));

/// The converters' rate.
const FS: f32 = 24_000.0;
/// log2(8 / 11).
const LOG2_8_11: f32 = -0.459_431_6;
/// The noise's seed: any non-zero word.
const SEED: u32 = 0x2545_f491;

/// The converters' bit depth: 16 at GRIT 0, 10 at 1.
pub fn bits(grit: f32) -> f32 {
    16.0 - 6.0 * clampf(grit, 0.0, 1.0)
}

/// One converter step, Δ_c = 2^(1 − b), over ±1.0.
pub fn step(grit: f32) -> f32 {
    exp2(1.0 - bits(grit))
}

/// The high cut's −3 dB point: 11 kHz at GRIT 0, 8 kHz at 1.
pub fn cutoff_hz(grit: f32) -> f32 {
    11_000.0 * exp2(clampf(grit, 0.0, 1.0) * LOG2_8_11)
}

/// The high cut: a one-pole lowpass by the bilinear transform, prewarped
/// so its −3 dB point is exactly `fc` (an RC's response, with its zero at
/// Nyquist). `y = b·(x + x₁) + p·y₁`, p = (1 − K)/(1 + K), b = (1 − p)/2,
/// K = tan(π·fc/fs). A plain `y += a·(x − y)` this close to Nyquist is a
/// shelf: forced to −3 dB at 11 kHz it already takes 1.4 dB at 5 kHz.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Cut {
    x1: f32,
    y1: f32,
}

impl Cut {
    /// (b, p) for `fc`.
    fn coefs(fc: f32) -> (f32, f32) {
        let t = 0.5 * fc / FS;
        let k = sin_turn(t) / sin_turn(t + 0.25);
        let p = (1.0 - k) / (1.0 + k);
        (0.5 * (1.0 - p), p)
    }

    /// Starts at rest on `x`: no step out of the bypass.
    fn prime(&mut self, x: f32) {
        (self.x1, self.y1) = (x, x);
    }

    #[inline(always)]
    fn run(&mut self, x: f32, (b, p): (f32, f32)) -> f32 {
        let y = b * (x + self.x1) + p * self.y1;
        (self.x1, self.y1) = (x, y);
        y
    }
}

/// One converter's idle mute: full while its input was non-zero within
/// `IDLE_HOLD` samples, then a linear fade over `IDLE_FADE`; any non-zero
/// input brings it back over `WAKE`. Zero bytes are asleep.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Idle {
    hold: u32,
    /// The dither's gain, in 1/`IDLE_FADE`.
    level: u32,
}

impl Idle {
    #[inline(always)]
    fn tick(&mut self, live: bool) -> f32 {
        if live || self.hold > 0 {
            self.hold = if live { IDLE_HOLD } else { self.hold - 1 };
            self.level = (self.level + WAKE_STEP).min(IDLE_FADE);
        } else {
            self.level = self.level.saturating_sub(1);
        }
        self.level as f32 * (1.0 / IDLE_FADE as f32)
    }

    fn gain(&self) -> f32 {
        self.level as f32 / IDLE_FADE as f32
    }
}

/// The ADC and the DAC. `Reverb` keeps one, and skips both while GRIT
/// eases at exactly 0. Zero bytes are `undithered()`, at rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Converters {
    /// The noise state, xorshift32, stepped twice a ring sample: the
    /// ADC's word's bytes 0–1 dither the ADC, the DAC's word's bytes 0–1
    /// its left and 2–3 its right. 0 is silent.
    noise: u32,
    /// The high cuts: ADC, DAC L, DAC R.
    cut: [Cut; 3],
    idle: [Idle; 2],
    /// Whether each side ran last block: a side waking from the bypass
    /// starts its high cut on its input, with no step.
    live: [bool; 2],
    /// The block settings, and !GRIT's bits they were made for (zero bytes
    /// match no GRIT).
    key: u32,
    bp: (f32, f32),
    delta: f32,
    inv: f32,
}

crate::in_place::field_list!(Converters => Converters { noise, cut, idle, live, key, bp, delta, inv });

impl Converters {
    pub fn new() -> Self {
        Self {
            noise: SEED,
            ..Self::undithered()
        }
    }

    /// Dither off: the quantisers alone, for tests.
    pub fn undithered() -> Self {
        Self {
            noise: 0,
            cut: [Cut::default(); 3],
            idle: [Idle::default(); 2],
            live: [false; 2],
            key: 0,
            bp: (0.0, 0.0),
            delta: 0.0,
            inv: 0.0,
        }
    }

    /// The ADC's and the DAC's dither gains.
    pub fn gains(&self) -> (f32, f32) {
        (self.idle[0].gain(), self.idle[1].gain())
    }

    /// Whether either side ran last block.
    pub fn live(&self) -> bool {
        self.live[0] || self.live[1]
    }

    /// GRIT is 0 this block: both sides rest, and the next to run starts
    /// its high cut afresh.
    pub fn bypass(&mut self) {
        self.live = [false; 2];
    }

    fn set(&mut self, grit: f32) {
        let grit = clampf(grit, 0.0, 1.0);
        if !grit.to_bits() != self.key {
            self.key = !grit.to_bits();
            self.bp = Cut::coefs(cutoff_hz(grit));
            self.delta = step(grit);
            self.inv = 1.0 / self.delta;
        }
    }

    /// The dither's scale: one step's TPDF per byte pair, none undithered.
    fn dither(&self) -> f32 {
        (self.noise != 0) as u32 as f32 * (1.0 / 256.0)
    }

    /// The decimated send, in dBFS units (1.0 = 0 dBFS).
    pub fn adc(&mut self, x: &mut [f32; HALF], grit: f32) {
        self.set(grit);
        if !self.live[0] {
            self.cut[0].prime(x[0]);
            self.live[0] = true;
        }
        let (bp, inv, delta, dither) = (self.bp, self.inv, self.delta, self.dither());
        let mut cut = self.cut[0];
        for x in x.iter_mut() {
            let w = xorshift32(&mut self.noise);
            let g = self.idle[0].tick(*x != 0.0) * dither;
            *x = rint(cut.run(*x, bp) * inv + tpdf(w as u16) * g) * delta;
        }
        self.cut[0] = cut;
    }

    /// The return, L and R, `unit` output dBFS units (after `WET`, before
    /// MIX) to one of theirs: the high cut is linear, so the quantiser's
    /// step carries the scale and the samples stay in the engine's units.
    pub fn dac(&mut self, l: &mut [f32; HALF], r: &mut [f32; HALF], grit: f32, unit: f32) {
        self.set(grit);
        if !self.live[1] {
            self.cut[1].prime(l[0]);
            self.cut[2].prime(r[0]);
            self.live[1] = true;
        }
        let (bp, dither) = (self.bp, self.dither());
        let (inv, delta) = (self.inv * unit, self.delta / unit);
        let (mut cl, mut cr) = (self.cut[1], self.cut[2]);
        for (l, r) in l.iter_mut().zip(r.iter_mut()) {
            let w = xorshift32(&mut self.noise);
            let g = self.idle[1].tick(*l != 0.0 || *r != 0.0) * dither;
            *l = rint(cl.run(*l, bp) * inv + tpdf(w as u16) * g) * delta;
            *r = rint(cr.run(*r, bp) * inv + tpdf((w >> 16) as u16) * g) * delta;
        }
        (self.cut[1], self.cut[2]) = (cl, cr);
    }
}

impl Default for Converters {
    fn default() -> Self {
        Self::new()
    }
}

/// Two bytes' difference: triangular over ±255, in 1/256 steps.
#[inline(always)]
fn tpdf(b: u16) -> f32 {
    (b & 0xff) as f32 - (b >> 8) as f32
}
