//! What every string loop is built from, so none can run away (ADR 0056).

/// A string loop's gain per pass: always below 1.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct LoopGain(f32);

impl LoopGain {
    pub const MAX: f32 = 0.9995;

    /// `g` clamped to `[0, MAX]`; NaN is 0.
    pub fn new(g: f32) -> Self {
        // Not `clamp`, which passes NaN.
        Self(if g >= 0.0 { g.min(Self::MAX) } else { 0.0 })
    }

    /// The gain that falls 60 dB in `t60_s` at `freq_hz` passes a second.
    pub fn from_t60(t60_s: f32, freq_hz: f32) -> Self {
        Self::new(libm::powf(0.001, 1.0 / (t60_s * freq_hz)))
    }

    pub fn get(self) -> f32 {
        self.0
    }

    pub fn min(self, o: Self) -> Self {
        Self(self.0.min(o.0))
    }
}

/// The DC blocker's corner.
pub const DC_HZ: f32 = 10.0;

/// A one-pole high-pass at `DC_HZ`, normalized so its gain is at most 1
/// at every frequency: `y = g·(x − x1) + r·y1`, `g = (1 + r) / 2`, so
/// `|H| = 1` at Nyquist and below it elsewhere.
pub struct DcBlocker {
    r: f32,
    g: f32,
    x1: f32,
    y1: f32,
}

impl DcBlocker {
    pub fn new(sample_rate: u32) -> Self {
        let r = libm::expf(-2.0 * core::f32::consts::PI * DC_HZ / sample_rate as f32);
        Self {
            r,
            g: (1.0 + r) * 0.5,
            x1: 0.0,
            y1: 0.0,
        }
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.g * (x - self.x1) + self.r * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    /// The pole, for the loop's phase delay.
    #[allow(dead_code)] // read by the string's tuning, next
    pub fn r(&self) -> f32 {
        self.r
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_gain_never_reaches_one() {
        for g in [
            LoopGain::new(1.0),
            LoopGain::new(2.0),
            LoopGain::from_t60(1e9, 49.0),
            LoopGain::new(f32::NAN),
        ] {
            assert!(g.get() <= LoopGain::MAX, "{g:?}");
        }
        assert_eq!(LoopGain::new(f32::NAN).get(), 0.0);
    }

    #[test]
    fn dc_blocker_gain_is_at_most_one() {
        let dc = DcBlocker::new(48_000);
        let (r, g) = (dc.r() as f64, dc.g as f64);
        for k in 0..512 {
            let w = core::f64::consts::PI * k as f64 / 511.0;
            // H = g·(1 − e^{−jω}) / (1 − r·e^{−jω})
            let (c, s) = (w.cos(), w.sin());
            let num = g * ((1.0 - c).powi(2) + s * s).sqrt();
            let den = ((1.0 - r * c).powi(2) + (r * s).powi(2)).sqrt();
            assert!(num / den <= 1.0 + 1e-6, "ω = {w}: {}", num / den);
        }
    }
}
