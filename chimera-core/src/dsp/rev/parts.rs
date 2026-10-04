//! What every reverb engine is built from (reverb TYPEs spec § Shared
//! machinery, § Level plan, § Moving a length, § Smoothing). Lines are
//! i16, full scale ±2.0 in line units; values passed in LSBs.

use core::f32::consts::{LOG2_E, TAU};

use chimera_hal::{BLOCK_SIZE, SAMPLE_RATE};

use crate::dsp::algo::math::exp2;
use crate::dsp::clampf;
use crate::dsp::halfband::HALF;
use crate::dsp::ring::rnd;

/// R1's re-entry clip: `x` to the knee at 1.0, then `1 + u − u²/4` with
/// `u = min(|x| − 1, 2)`, reaching 2.0 with slope 0 at |x| = 3.
/// Branch-free; NaN comes out ±1.
#[inline(always)]
pub fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    let u = clampf(a - 1.0, 0.0, 2.0);
    (a.min(1.0) + u - 0.25 * u * u).copysign(x)
}

/// R2's in-loop allpass on its normalised state: `w_d` is the stored
/// `(1 − g)·v` from M samples ago, `inv_1mg` = 1/(1 − g) once a block.
/// Returns `(y, w)`; the caller truncates `w` into the line.
#[inline(always)]
pub fn ap_norm(x: f32, w_d: f32, g: f32, inv_1mg: f32) -> (f32, f32) {
    let z = w_d * inv_1mg;
    let v = x - g * z;
    (z + g * v, (1.0 - g) * v)
}

/// The plain allpass on an f32 state `z` (R3's diffusers, R4's
/// decorrelators): `v = x − g·z`, `y = z + g·v`. Returns `(y, v)`.
#[inline(always)]
pub fn ap_f32(x: f32, z: f32, g: f32) -> (f32, f32) {
    let v = x - g * z;
    (z + g * v, v)
}

/// The wobble's moving read: `d` (≥ 2) samples behind `base` in `lines`,
/// a linear pair `x0 + f·(x1 − x0)`, both slots wrapped. In LSBs.
#[inline(always)]
pub fn pair(lines: &[i16], base: usize, d: f32) -> f32 {
    debug_assert!(d >= 2.0 && (d as usize) < lines.len() && base < lines.len());
    let n = lines.len();
    let di = d as usize;
    let f = d - di as f32;
    let wrap = |i: usize| if i >= n { i - n } else { i };
    let i0 = wrap(base + n - di);
    let i1 = wrap(i0 + n - 1);
    let (x0, x1) = (lines[i0] as f32, lines[i1] as f32);
    x0 + f * (x1 - x0)
}

/// Ring samples a length crossfade takes: 30 ms, QUAD's.
pub const XFADE: u16 = 720;
const XFADE_INV: f32 = 1.0 / XFADE as f32;

/// A pitch-free length move: fixed reads at `from` and `to`, blended over
/// `XFADE` ring samples. Zero bytes are idle at length 0.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Xfade {
    pub from: u16,
    pub to: u16,
    left: u16,
}

impl Xfade {
    /// Head for `to`; a request during a crossfade waits (QUAD's rule).
    pub fn start(&mut self, to: u16) {
        if self.left == 0 && to != self.to {
            (self.to, self.left) = (to, XFADE);
        }
    }

    /// `to`'s share of the blend.
    #[inline(always)]
    pub fn weight(&self) -> f32 {
        (XFADE - self.left) as f32 * XFADE_INV
    }

    /// One ring sample on; the last lands `from` on `to`.
    #[inline(always)]
    pub fn tick(&mut self) {
        if self.left > 0 {
            self.left -= 1;
            if self.left == 0 {
                self.from = self.to;
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.left > 0
    }
}

/// The wobble LFO: a `u32` phase at the ring rate, read once a block and
/// lerped across its `HALF` ring samples by the engine. Zero bytes stop it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wobble {
    phase: u32,
    inc: u32,
}

impl Wobble {
    /// `hz` at the ring rate `fs`.
    pub fn set_rate(&mut self, hz: f32, fs: f32) {
        self.inc = (clampf(hz / fs, 0.0, 0.5) * 4_294_967_296.0 + 0.5) as u32;
    }

    /// This block's start, L and R 90° apart (L leads), in −1..1; then a
    /// block on.
    pub fn block(&mut self) -> [f32; 2] {
        let turn = self.phase as f32 * (1.0 / 4_294_967_296.0);
        self.phase = self.phase.wrapping_add(self.inc.wrapping_mul(HALF as u32));
        [sin_turn(turn), sin_turn(turn - 0.25)]
    }
}

/// sin(2π·`t`), `t` in −1..1, to under 4e-6: folded to a quarter turn,
/// then Taylor to the 9th power. Not `fast_sin_turns`: `SINE_LUT` is built
/// by a Taylor series over ±π with no quarter-wave fold, so its entries
/// err up to 5e-4 near a half turn, past the lerp's 1e-4 budget. Goes
/// when private issue #352 fixes the table.
#[inline(always)]
pub(crate) fn sin_turn(t: f32) -> f32 {
    let mut x = t - (t as i32) as f32;
    x -= (x > 0.5) as i32 as f32;
    x += (x < -0.5) as i32 as f32;
    let a = x.abs();
    let th = TAU * a.min(0.5 - a).copysign(x);
    let t2 = th * th;
    th * (1.0 - t2 / 6.0 * (1.0 - t2 / 20.0 * (1.0 - t2 / 42.0 * (1.0 - t2 / 72.0))))
}

/// The per-block one-pole every continuous knob eases by, as
/// `dsp::ease::Ease::step`'s `k`: the share kept each 64-sample block at
/// 48 kHz, for a time constant of `tau_s`.
pub fn ease_k(tau_s: f32) -> f32 {
    exp2(-LOG2_E * BLOCK_SIZE as f32 / (tau_s * SAMPLE_RATE as f32))
}

/// `y += a·(x − y)`'s `a` for a corner at `fc` and rate `fs`.
pub fn one_pole(fc: f32, fs: f32) -> f32 {
    1.0 - exp2(-LOG2_E * TAU * fc / fs)
}

/// A delay or damper write: `x` (LSBs) rounded, ties away from zero, and
/// saturated to i16.
#[inline(always)]
pub fn store_round(x: f32) -> i16 {
    saturate(rnd(x))
}

/// An allpass state or REGEN write: `x` (LSBs) truncated toward zero, and
/// saturated to i16. Truncation always decays.
#[inline(always)]
pub fn store_trunc(x: f32) -> i16 {
    saturate(x as i32)
}

#[inline(always)]
fn saturate(r: i32) -> i16 {
    let s = r.clamp(i16::MIN as i32, i16::MAX as i32);
    #[cfg(any(test, feature = "test-support"))]
    probe::note(s != r);
    s as i16
}

/// Clamp hits and writes through `store_*` on this thread since the last
/// reset (§ Level plan's transients). Host tests only.
#[cfg(any(test, feature = "test-support"))]
pub fn clamp_counts() -> (u64, u64) {
    probe::COUNTS.with(|c| c.get())
}

#[cfg(any(test, feature = "test-support"))]
pub fn reset_clamp_counts() {
    probe::COUNTS.with(|c| c.set((0, 0)));
}

#[cfg(any(test, feature = "test-support"))]
mod probe {
    extern crate std;
    use core::cell::Cell;

    std::thread_local! {
        /// (hits, writes): per thread, so parallel tests don't mix.
        pub static COUNTS: Cell<(u64, u64)> = const { Cell::new((0, 0)) };
    }

    pub fn note(hit: bool) {
        COUNTS.with(|c| {
            let (h, w) = c.get();
            c.set((h + hit as u64, w + 1));
        });
    }
}
