// The dispersion curve follows Mutable Instruments Rings' `ap_gain` (ADR 0032):
//
// Copyright 2014-2015 Emilie Gillet.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.

//! STRUCTURE's stiffness on STRING: a chain of first-order allpasses in
//! the loop, delaying the low partials more than the high ones, so the
//! partials stretch as in a stiff string (ADR 0056).

use super::loop_parts::{Allpass1, allpass_phase_delay};

pub const DISPERSION_STAGES: usize = 4;

/// The chain's DC delay at STRUCTURE 1, over its flat `STAGES`: this
/// share of the period. The 8th partial of C3 then sits about 24 cents
/// sharp, rising with pitch, as on piano wire.
const SPAN: f32 = 0.1;

#[derive(Default)]
pub struct Dispersion {
    stages: [Allpass1; DISPERSION_STAGES],
}

impl Dispersion {
    /// Each stage's coefficient at `structure` on a loop of `period`
    /// samples. Rings' `ap_gain` curve, `s / (0.15 + s)`, sets how much of
    /// `SPAN` the stages' DC delay takes: 1 sample each at 0 (a plain
    /// delay), `1 + SPAN·period/STAGES` at 1. Rings scales an allpass line
    /// with the period; one first-order stage per quarter scales its
    /// corner instead. The chain's DC delay is at most half the period.
    pub fn coeff(structure: f32, period: f32) -> f32 {
        let s = structure.clamp(0.0, 1.0);
        let curve = s / (0.15 + s) * 1.15;
        let stages = DISPERSION_STAGES as f32;
        let dc = 1.0 + curve * SPAN * period / stages;
        let limit = period / (2.0 * stages);
        ((1.0 - dc) / (1.0 + dc)).max((1.0 - limit) / (1.0 + limit))
    }

    pub fn set(&mut self, a: f32) {
        for s in &mut self.stages {
            s.set(a);
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.stages.iter_mut().fold(x, |x, s| s.process(x))
    }

    pub fn reset(&mut self) {
        for s in &mut self.stages {
            s.reset();
        }
    }

    /// The chain's phase delay at `w` rad/sample, in samples.
    pub fn phase_delay(a: f32, w: f32) -> f32 {
        DISPERSION_STAGES as f32 * allpass_phase_delay(a, w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coeff_is_rings_curve_within_the_period_limit() {
        assert_eq!(Dispersion::coeff(0.0, 979.0), 0.0);
        let dc = 1.0 + SPAN * 979.0 / 4.0;
        let want = (1.0 - dc) / (1.0 + dc);
        assert!((Dispersion::coeff(1.0, 979.0) - want).abs() < 1e-6);
        // Rings' curve: 0.575 of its reach by STRUCTURE 0.15.
        let dc = |s| {
            let a = Dispersion::coeff(s, 979.0);
            (1.0 - a) / (1.0 + a)
        };
        let share = (dc(0.15) - dc(0.0)) / (dc(1.0) - dc(0.0));
        assert!((share - 0.575).abs() < 1e-3, "{share}");
        for p in [6.0, 22.9, 979.0] {
            let d = Dispersion::phase_delay(Dispersion::coeff(1.0, p), 1e-4);
            assert!(d <= p / 2.0 + 1e-3, "{p}: {d}");
        }
    }

    #[test]
    fn coeff_stiffens_with_structure() {
        for p in [22.9, 183.0, 979.0] {
            let mut last = 1.0;
            for k in 0..=20 {
                let a = Dispersion::coeff(k as f32 / 20.0, p);
                assert!(a <= last && a > -1.0, "{p}: {a}");
                last = a;
            }
        }
    }

    /// The chain's measured phase delay at `w` is `phase_delay`'s.
    #[test]
    fn phase_delay_is_the_chains() {
        let a = Dispersion::coeff(1.0, 367.0);
        let w = core::f32::consts::TAU / 367.0;
        let mut d = Dispersion::default();
        d.set(a);
        // A sine through the chain, settled: its lag in samples.
        let (mut re, mut im) = (0.0_f64, 0.0_f64);
        for n in 0..40_000 {
            let y = d.process(libm::sinf(w * n as f32)) as f64;
            if n >= 20_000 {
                let t = w as f64 * n as f64;
                (re, im) = (re + y * t.sin(), im + y * t.cos());
            }
        }
        let lag = -(im.atan2(re)) / w as f64;
        let lag = lag.rem_euclid(367.0);
        let want = Dispersion::phase_delay(a, w) as f64;
        assert!((lag - want).abs() < 0.05, "{lag} vs {want}");
    }
}
