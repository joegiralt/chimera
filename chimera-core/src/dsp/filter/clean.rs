//! CLEAN: the TPT state-variable filter with `sat_c1` on both states, in
//! the feedback loop, so it bends instead of blowing up.
//!
//! Coefficients are set once a block (spec § 2, B1). A steady block, `g`
//! and `k` equal at both ends, divides once for the block; a moving one
//! ramps `g` and `k` and divides exactly every sample. `d` is convex in
//! `g`, so lerping it is wrong by up to 95 % on a one-block jump. Each
//! stage takes its `d`, and never divides.

use super::coef::{g_of, sat_c1};
use super::{FilterMode, g_at};
use crate::hw::BLOCK_SIZE;
use crate::params::FilterParams;

#[derive(Clone, Debug, Default)]
pub struct Clean {
    s1: [f32; 2],
    s2: [f32; 2],
    /// The last block's `g`, `k` and DRIVE gain; `None` on a fresh voice,
    /// which starts unramped.
    g: Option<f32>,
    k: Option<f32>,
    pre: Option<f32>,
}

/// One block's `(from, to)` for `g`, `k` and the DRIVE gain.
#[derive(Clone, Copy)]
pub(super) struct Ramps {
    g: (f32, f32),
    k: (f32, f32),
    pre: (f32, f32),
}

/// A sample's coefficients: `d = 1/(1 + kg + g²)` for stage 1, and `d2`
/// at `k2` for stage 2 (LP6's `d` is `1/(1 + g)`).
#[derive(Clone, Copy)]
struct Coef {
    g: f32,
    k: f32,
    k2: f32,
    d: f32,
    d2: f32,
}

/// Mode `M`'s coefficients at `g` and `k`: a divide a stage, and only
/// those `M` reads.
#[inline(always)]
fn coef<const M: u8>(g: f32, k: f32) -> Coef {
    let d_at = |k: f32| 1.0 / (1.0 + k * g + g * g);
    let mut c = Coef {
        g,
        k,
        k2: k,
        d: 0.0,
        d2: 0.0,
    };
    match FilterMode::ALL[M as usize] {
        FilterMode::Lp6 => c.d = 1.0 / (1.0 + g),
        FilterMode::Lp24 | FilterMode::Bp24 | FilterMode::Hp24 => {
            // The second stage less resonant, so the cascade doesn't blow up.
            c.k2 = k * 1.2 + 0.3;
            c.d = d_at(k);
            c.d2 = d_at(c.k2);
        }
        FilterMode::Phaser => {
            c.d = d_at(k);
            c.d2 = c.d;
        }
        FilterMode::Lp12 | FilterMode::Bp12 | FilterMode::Notch => c.d = d_at(k),
    }
    c
}

fn ramp(last: &mut Option<f32>, to: f32) -> (f32, f32) {
    (last.replace(to).unwrap_or(to), to)
}

impl Clean {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process(&mut self, buf: &mut [f32; BLOCK_SIZE], p: &FilterParams, fs: u32) {
        let r = self.ramps(p, fs);
        // The mode and the loop matched once a block: each is its own loop.
        match p.mode() {
            FilterMode::Lp6 => self.run::<0>(buf, r),
            FilterMode::Lp12 => self.run::<1>(buf, r),
            FilterMode::Lp24 => self.run::<2>(buf, r),
            FilterMode::Bp12 => self.run::<3>(buf, r),
            FilterMode::Bp24 => self.run::<4>(buf, r),
            FilterMode::Hp24 => self.run::<5>(buf, r),
            FilterMode::Notch => self.run::<6>(buf, r),
            FilterMode::Phaser => self.run::<7>(buf, r),
        }
    }

    /// This block's ramps, stored as the next block's start.
    #[inline(always)]
    pub(super) fn ramps(&mut self, p: &FilterParams, fs: u32) -> Ramps {
        // k = 2 (none) to 0.01; the law moves to the spec's in Task 3.
        let k = 2.0 * (1.0 - p.resonance) + 0.01;
        Ramps {
            g: ramp(&mut self.g, g_of(p.cutoff, fs as f32)),
            k: ramp(&mut self.k, k),
            pre: ramp(&mut self.pre, 1.0 + p.drive * 4.0),
        }
    }

    #[inline(always)]
    fn run<const M: u8>(&mut self, buf: &mut [f32; BLOCK_SIZE], r: Ramps) {
        if r.g.0 == r.g.1 && r.k.0 == r.k.1 {
            self.steady::<M>(buf, r);
        } else {
            self.moving::<M>(buf, r);
        }
    }

    /// `g` and `k` held: their divides once a block. DRIVE still ramps (a
    /// multiply).
    #[inline(always)]
    pub(super) fn steady<const M: u8>(&mut self, buf: &mut [f32; BLOCK_SIZE], r: Ramps) {
        let c = coef::<M>(r.g.1, r.k.1);
        let pre_step = (r.pre.1 - r.pre.0) / BLOCK_SIZE as f32;
        for (i, s) in buf.iter_mut().enumerate() {
            *s = self.tick::<M>(*s * g_at(r.pre.0, pre_step, i), &c);
        }
    }

    /// `g`, `k` and DRIVE ramp to this block's values, reached on the last
    /// sample (#53), and every sample divides exactly.
    #[inline(always)]
    pub(super) fn moving<const M: u8>(&mut self, buf: &mut [f32; BLOCK_SIZE], r: Ramps) {
        let n = BLOCK_SIZE as f32;
        let (g_step, k_step) = ((r.g.1 - r.g.0) / n, (r.k.1 - r.k.0) / n);
        let pre_step = (r.pre.1 - r.pre.0) / n;
        for (i, s) in buf.iter_mut().enumerate() {
            let c = coef::<M>(g_at(r.g.0, g_step, i), g_at(r.k.0, k_step, i));
            *s = self.tick::<M>(*s * g_at(r.pre.0, pre_step, i), &c);
        }
    }

    /// Forget the last `g`, `k` and DRIVE: the next block starts unramped.
    pub fn hold(&mut self) {
        (self.g, self.k, self.pre) = (None, None, None);
    }

    /// The last block's `g` (`None`: no ramp next block). Test-only: nothing
    /// in the audio path needs to read the ramp state back.
    #[cfg(test)]
    pub(crate) fn last_g(&self) -> Option<f32> {
        self.g
    }

    #[inline(always)]
    fn tick<const M: u8>(&mut self, x: f32, c: &Coef) -> f32 {
        match FilterMode::ALL[M as usize] {
            FilterMode::Lp6 => {
                let v = (x - self.s1[0]) * c.g * c.d;
                let lp = v + self.s1[0];
                self.s1[0] = lp + v;
                lp
            }
            FilterMode::Lp12 => self.stage(x, c.g, c.k, c.d, 0).0,
            FilterMode::Lp24 => {
                let lp = self.stage(x, c.g, c.k, c.d, 0).0;
                self.stage(lp, c.g, c.k2, c.d2, 1).0
            }
            // BP boosted for presence.
            FilterMode::Bp12 => self.stage(x, c.g, c.k, c.d, 0).1 * 2.0,
            FilterMode::Bp24 => {
                let bp = self.stage(x, c.g, c.k, c.d, 0).1;
                self.stage(bp, c.g, c.k2, c.d2, 1).1 * 2.0
            }
            FilterMode::Hp24 => {
                let hp = self.stage(x, c.g, c.k, c.d, 0).2;
                self.stage(hp, c.g, c.k2, c.d2, 1).2
            }
            FilterMode::Notch => {
                let (lp, _, hp) = self.stage(x, c.g, c.k, c.d, 0);
                lp + hp
            }
            FilterMode::Phaser => {
                let ap1 = x - 2.0 * c.k * self.stage(x, c.g, c.k, c.d, 0).1;
                ap1 - 2.0 * c.k * self.stage(ap1, c.g, c.k, c.d2, 1).1
            }
        }
    }

    /// One SVF stage, `d = 1/(1 + kg + g²)` given: `(lp, bp, hp)`.
    #[inline(always)]
    fn stage(&mut self, x: f32, g: f32, k: f32, d: f32, i: usize) -> (f32, f32, f32) {
        let v1 = sat_c1(self.s1[i]);
        let v2 = sat_c1(self.s2[i]);
        let hp = (x - (k + g) * v1 - v2) * d;
        let bp = g * hp + v1;
        let lp = g * bp + v2;
        self.s1[i] = bp + g * hp;
        self.s2[i] = lp + g * bp;
        (lp, bp, hp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hw::SAMPLE_RATE;

    /// The moving loop on a steady block is the steady loop: the same
    /// coefficients, per sample instead of once.
    #[test]
    fn steady_and_moving_agree_on_a_steady_block() {
        let mut p = FilterParams::default();
        (p.cutoff, p.resonance) = (1500.0, 0.6);
        let saw = |b: usize| -> [f32; BLOCK_SIZE] {
            core::array::from_fn(|i| 0.8 * (((b * BLOCK_SIZE + i) % 218) as f32 / 109.0 - 1.0))
        };
        let mut a = Clean::new();
        for b in 0..3 {
            a.process(&mut saw(b), &p, SAMPLE_RATE);
        }
        let mut m = a.clone();
        let (mut s, mut v) = (saw(3), saw(3));
        let r = a.ramps(&p, SAMPLE_RATE);
        a.steady::<2>(&mut s, r);
        let r = m.ramps(&p, SAMPLE_RATE);
        m.moving::<2>(&mut v, r);
        let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.01);
        for (i, (a, b)) in s.iter().zip(&v).enumerate() {
            assert!((a - b).abs() <= 1e-6 * peak, "sample {i}: {a} vs {b}");
        }
    }
}
