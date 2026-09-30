//! STRING's and SYMP's ensemble (ours, #50): three read heads on the
//! string's own line, swung by one quadrature LFO at 0°, 90° and 180°,
//! mixed on the output. The heads' Doppler is the detune. Not 120°: three
//! even phases cancel each partial's first sidebands in the sum
//! (Σ e^(i2πk/3) = 0), and the mix barely moves.

use core::f32::consts::TAU;

/// Read heads.
pub const ENS_HEADS: usize = 3;
/// A head's peak detune at DEPTH 1, cents, where the loop lets it swing.
pub const ENS_MAX_CENTS: f32 = 15.0;

/// ENS RATE's LFO, Hz: 0.1 · 60^rate, 0.1 to 6.
pub fn rate_hz(ens_rate: f32) -> f32 {
    0.1 * libm::powf(60.0, ens_rate)
}

/// A quadrature LFO `(cos, sin)`, rotated a sample at a time, and the
/// heads' swing, samples.
pub struct Ensemble {
    cos: f32,
    sin: f32,
    rot_c: f32,
    rot_s: f32,
    amp: f32,
}

impl Default for Ensemble {
    fn default() -> Self {
        Self {
            cos: 1.0,
            sin: 0.0,
            rot_c: 1.0,
            rot_s: 0.0,
            amp: 0.0,
        }
    }
}

/// The most a head may swing about `delay / 2`: none under 4 samples (F7).
fn cap(delay: usize) -> f32 {
    (delay as f32 * 0.5 - 2.0).max(0.0)
}

impl Ensemble {
    /// Per block: the LFO at `rate` Hz, renormalised, and the swing that
    /// peaks at `depth · ENS_MAX_CENTS`, capped to the loop.
    pub fn set(&mut self, depth: f32, rate: f32, delay: usize, sample_rate: u32) {
        let w = TAU * rate / sample_rate as f32;
        (self.rot_c, self.rot_s) = (libm::cosf(w), libm::sinf(w));
        let g = 1.0 / libm::sqrtf(self.cos * self.cos + self.sin * self.sin);
        (self.cos, self.sin) = (self.cos * g, self.sin * g);
        let detune = libm::exp2f(ENS_MAX_CENTS * depth / 1200.0) - 1.0;
        self.amp = (detune / w).min(cap(delay));
    }

    /// Head k's delay behind the write, this sample: within [2, delay − 2],
    /// or `delay / 2` under 4 samples.
    #[inline]
    pub fn head_delays(&self, delay: usize) -> [f32; ENS_HEADS] {
        let (mid, cap) = (delay as f32 * 0.5, cap(delay));
        // Clamped: the LFO's radius drifts past 1 by rounding.
        [self.sin, self.cos, -self.sin].map(|s| (mid + self.amp * s).clamp(mid - cap, mid + cap))
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

    /// Every head stays in `[2, delay − 2]`, or at `delay / 2` on a loop
    /// too short to swing (F7).
    #[test]
    fn heads_stay_inside_the_loop() {
        for delay in [3, 23, 1010] {
            for depth in [0.0, 1.0] {
                for rate in [0.0, 1.0] {
                    let mut e = Ensemble::default();
                    e.set(depth, rate_hz(rate), delay, SR);
                    for i in 0..100_000 {
                        if i % 32 == 0 {
                            e.set(depth, rate_hz(rate), delay, SR);
                        }
                        for o in e.head_delays(delay) {
                            if delay == 3 {
                                assert_eq!(o, 1.5, "{depth} {rate}");
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
            let mut e = Ensemble::default();
            let hz = rate_hz(0.5);
            let mut last = e.head_delays(900)[0];
            let mut most = 0.0_f32;
            for i in 0..SR as usize * 2 {
                if i % 32 == 0 {
                    e.set(depth, hz, 900, SR);
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
