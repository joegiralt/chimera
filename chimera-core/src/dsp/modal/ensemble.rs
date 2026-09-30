//! STRING's and SYMP's ensemble (ours, #50): two read heads on the
//! string's own line, near the write, swung by one quadrature LFO at 0°
//! and 90°, mixed on the output. The heads' Doppler is the detune. Not
//! three at 120°, nor any pair at 180°: evenly spread phases cancel each
//! partial's first sidebands in the sum (Σ e^(i2πk/3) = 0), and the mix
//! barely moves.

use core::f32::consts::TAU;

/// Read heads.
pub const ENS_HEADS: usize = 2;
/// A head's peak detune at DEPTH 1, cents, where the loop lets it swing.
pub const ENS_MAX_CENTS: f32 = 15.0;

/// ENS RATE's LFO, Hz: 0.1 · 60^rate, 0.1 to 6.
pub fn rate_hz(ens_rate: f32) -> f32 {
    0.1 * libm::powf(60.0, ens_rate)
}

/// A quadrature LFO `(cos, sin)`, rotated a sample at a time, and the
/// heads' swing, samples: `swing` as the note set it, `amp` as the loop
/// caps it.
#[derive(Clone, Copy)]
pub struct Ensemble {
    cos: f32,
    sin: f32,
    rot_c: f32,
    rot_s: f32,
    swing: f32,
    amp: f32,
}

impl Default for Ensemble {
    fn default() -> Self {
        Self {
            cos: 1.0,
            sin: 0.0,
            rot_c: 1.0,
            rot_s: 0.0,
            swing: 0.0,
            amp: 0.0,
        }
    }
}

/// The most a head may swing: `2 + 2·cap ≤ delay − 2`, none under 4
/// samples (F7).
fn cap(delay: usize) -> f32 {
    ((delay as f32 - 4.0) * 0.5).max(0.0)
}

/// The farthest a head sits behind the write.
fn top(delay: usize) -> f32 {
    2.0 + 2.0 * cap(delay)
}

impl Ensemble {
    /// At note-on: the LFO at `hz` (> 0) from phase 0, and the swing whose
    /// Doppler peaks at `depth · ENS_MAX_CENTS`.
    pub fn new(depth: f32, hz: f32, sample_rate: u32) -> Self {
        let w = TAU * hz / sample_rate as f32;
        Self {
            rot_c: libm::cosf(w),
            rot_s: libm::sinf(w),
            swing: (libm::exp2f(ENS_MAX_CENTS * depth / 1200.0) - 1.0) / w,
            ..Self::default()
        }
    }

    /// Per block: the LFO renormalised, the swing capped to the loop.
    pub fn set(&mut self, delay: usize) {
        let g = 1.0 / libm::sqrtf(self.cos * self.cos + self.sin * self.sin);
        (self.cos, self.sin) = (self.cos * g, self.sin * g);
        self.amp = self.swing.min(cap(delay));
    }

    /// Head k's delay behind the write, this sample: `2 + A + A·sₖ`, in
    /// phase with the dry at DEPTH 0, within [2, delay − 2] (2 under 4).
    #[inline]
    pub fn head_delays(&self, delay: usize) -> [f32; ENS_HEADS] {
        let (mid, top) = (2.0 + self.amp, top(delay));
        // Clamped: the LFO's radius drifts past 1 by rounding. Not `clamp`
        // or `min`, whose checks run every sample; no head is NaN.
        [self.sin, self.cos].map(|s| {
            let o = (mid + self.amp * s).max(2.0);
            if o > top { top } else { o }
        })
    }

    /// Past the oldest sample a head reads behind the write this block, on
    /// a line of `delay`: `set` holds the LFO's radius to 1 within 1e-5, so
    /// no head passes `2 + 2.001·A`, and one more sample covers rounding.
    pub fn reach(&self, delay: usize) -> usize {
        (2.0 + 2.001 * self.amp).min(top(delay)) as usize + 2
    }

    #[inline]
    pub fn advance(&mut self) {
        (self.cos, self.sin) = (
            self.cos * self.rot_c - self.sin * self.rot_s,
            self.sin * self.rot_c + self.cos * self.rot_s,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// Every head stays in `[2, delay − 2]`, or at 2 on a loop too short to
    /// swing (F7), and its older tap short of `reach`.
    #[test]
    fn heads_stay_inside_the_loop() {
        for delay in [3, 23, 1010] {
            for depth in [0.0, 1.0] {
                for rate in [0.0, 1.0] {
                    let mut e = Ensemble::new(depth, rate_hz(rate), SR);
                    for i in 0..100_000 {
                        if i % 32 == 0 {
                            e.set(delay);
                        }
                        for o in e.head_delays(delay) {
                            assert!(o as usize + 1 < e.reach(delay), "{delay}: {o}");
                            if delay == 3 {
                                assert_eq!(o, 2.0, "{depth} {rate}");
                            } else {
                                assert!(
                                    (2.0..=delay as f32 - 2.0).contains(&o),
                                    "{delay} {depth} {rate}: {o}"
                                );
                            }
                        }
                        e.advance();
                    }
                }
            }
        }
    }

    /// Uncapped, a head's Doppler peaks at `ENS_MAX_CENTS` at DEPTH 1, and
    /// half that at DEPTH 0.5.
    #[test]
    fn the_doppler_is_the_detune() {
        for (depth, want) in [(1.0, ENS_MAX_CENTS), (0.5, ENS_MAX_CENTS * 0.5)] {
            let mut e = Ensemble::new(depth, rate_hz(0.5), SR);
            e.set(900);
            let mut last = e.head_delays(900)[0];
            let mut most = 0.0_f32;
            for i in 0..SR as usize * 2 {
                if i % 32 == 0 {
                    e.set(900);
                }
                let o = e.head_delays(900)[0];
                // A delay falling by `d` a sample reads `1 + d` fast.
                most = most.max(1200.0 * libm::log2f(1.0 + (last - o)));
                last = o;
                e.advance();
            }
            assert!((most - want).abs() < 0.1, "{depth}: {most} cents");
        }
    }

    #[test]
    fn rate_spans_a_tenth_to_six_hz() {
        assert!((rate_hz(0.0) - 0.1).abs() < 1e-6);
        assert!((rate_hz(1.0) - 6.0).abs() < 1e-4);
    }
}
