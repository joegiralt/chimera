//! BODY on STRING and SYMP: three fixed resonances on the output, outside
//! the loop, so they colour the note without moving it (#10, ADR 0056).

use super::rings::Svf;

/// BODY's dry and wet gains (`Body::mix`).
#[derive(Clone, Copy)]
pub struct BodyMix {
    dry: f32,
    wet: f32,
}

/// Each resonance: Hz, Q, gain.
pub const BODY_MODES: [(f32, f32, f32); 3] =
    [(102.0, 3.0, 1.0), (236.0, 4.0, 0.7), (517.0, 3.0, 0.5)];

pub struct Body {
    modes: [Svf; 3],
}

impl Body {
    /// Tuned at `sample_rate`, once: the resonances are fixed.
    pub fn new(sample_rate: u32) -> Self {
        let mut modes: [Svf; 3] = core::array::from_fn(|_| Svf::new());
        for (m, (hz, q, _)) in modes.iter_mut().zip(BODY_MODES) {
            m.set(hz / sample_rate as f32, q);
        }
        Self { modes }
    }

    /// Silent, as at a note-on.
    pub fn reset(&mut self) {
        for m in &mut self.modes {
            m.reset();
        }
    }

    /// BODY `amount`'s dry and wet gains, `(x + amount·wet) / (1 +
    /// amount/2)` as two products: once a note, not a divide a sample.
    pub fn mix(amount: f32) -> BodyMix {
        let n = 1.0 / (1.0 + 0.5 * amount);
        BodyMix {
            dry: n,
            wet: amount * n,
        }
    }

    /// `x` through the body at `mix`. Each band-pass peaks at its gain,
    /// and the sum is scaled back so the loudest peak stays under 2.
    #[inline]
    pub fn process(&mut self, x: f32, mix: BodyMix) -> f32 {
        if mix.wet == 0.0 {
            return x;
        }
        let mut wet = 0.0;
        for (m, (_, _, g)) in self.modes.iter_mut().zip(BODY_MODES) {
            wet += g * m.process_bp_normalized(x);
        }
        mix.dry * x + mix.wet * wet
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    const SR: u32 = 48_000;

    fn impulse_response(amount: f32) -> Vec<f32> {
        let mut b = Body::new(SR);
        (0..16_384)
            .map(|n| b.process(if n == 0 { 1.0 } else { 0.0 }, Body::mix(amount)))
            .collect()
    }

    /// |H| at `hz`, from the impulse response.
    fn gain_at(h: &[f32], hz: f64) -> f64 {
        let w = core::f64::consts::TAU * hz / SR as f64;
        let (re, im) = h.iter().enumerate().fold((0.0, 0.0), |(re, im), (n, &x)| {
            let t = w * n as f64;
            (re + x as f64 * t.cos(), im - x as f64 * t.sin())
        });
        re.hypot(im)
    }

    #[test]
    fn body_gain_is_bounded() {
        let h = impulse_response(1.0);
        let mut peak: f64 = 0.0;
        for k in 0..=600 {
            let hz = 20.0 * 1000f64.powf(k as f64 / 600.0);
            peak = peak.max(gain_at(&h, hz));
        }
        assert!(peak <= 2.0, "peak {peak}");
        // It colours: the first resonance stands above the dry level.
        assert!(gain_at(&h, 102.0) > 1.2, "{}", gain_at(&h, 102.0));
    }

    #[test]
    fn body_at_zero_is_dry() {
        let h = impulse_response(0.0);
        assert_eq!(h[0], 1.0);
        assert!(h[1..].iter().all(|&x| x == 0.0));
    }
}
