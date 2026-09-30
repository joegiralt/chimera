//! A one-pole high-pass for DC: on the string models' output (10 Hz,
//! ADR 0056) and on every voice after its last nonlinear stage (ADR 0060).

/// A one-pole high-pass at a corner, normalized so its gain is at most 1
/// at every frequency: `y = g·(x − x1) + r·y1`, `g = (1 + r) / 2`, so
/// `|H| = 1` at Nyquist and below it elsewhere.
#[derive(Clone, Copy, Debug)]
pub struct DcBlocker {
    r: f32,
    g: f32,
    x1: f32,
    y1: f32,
}

impl DcBlocker {
    pub fn new(corner_hz: f32, sample_rate: u32) -> Self {
        let r = libm::expf(-2.0 * core::f32::consts::PI * corner_hz / sample_rate as f32);
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

    /// `process` over a block, the state in registers.
    #[inline]
    pub fn run(&mut self, buf: &mut [f32]) {
        let (g, r) = (self.g, self.r);
        let (mut x1, mut y1) = (self.x1, self.y1);
        for s in buf.iter_mut() {
            let x = *s;
            y1 = g * (x - x1) + r * y1;
            x1 = x;
            *s = y1;
        }
        (self.x1, self.y1) = (x1, y1);
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// |H(ω)| at `w` of a blocker at `hz`.
    fn gain(hz: f32, w: f64) -> f64 {
        let dc = DcBlocker::new(hz, 48_000);
        let (r, g) = (dc.r as f64, dc.g as f64);
        // H = g·(1 − e^{−jω}) / (1 − r·e^{−jω})
        let (c, s) = (w.cos(), w.sin());
        let num = g * ((1.0 - c).powi(2) + s * s).sqrt();
        let den = ((1.0 - r * c).powi(2) + (r * s).powi(2)).sqrt();
        num / den
    }

    #[test]
    fn dc_blocker_gain_is_at_most_one() {
        for hz in [5.0, 10.0] {
            for k in 0..512 {
                let w = core::f64::consts::PI * k as f64 / 511.0;
                assert!(
                    gain(hz, w) <= 1.0 + 1e-6,
                    "{hz} Hz, ω = {w}: {}",
                    gain(hz, w)
                );
            }
        }
    }

    /// The voice's corner keeps the bass: within 0.3 dB at 20 Hz, 0.07 dB
    /// at E1 (41 Hz).
    #[test]
    fn five_hz_keeps_the_bass() {
        let db = |hz: f64| 20.0 * gain(5.0, core::f64::consts::TAU * hz / 48_000.0).log10();
        assert!(db(20.0) > -0.3, "{}", db(20.0));
        assert!(db(41.2) > -0.07, "{}", db(41.2));
    }

    #[test]
    fn run_is_process_bit_for_bit() {
        let (mut a, mut b) = (DcBlocker::new(5.0, 48_000), DcBlocker::new(5.0, 48_000));
        let mut buf: [f32; 64] = core::array::from_fn(|i| 0.3 + (i as f32 * 0.21).sin());
        let want: [f32; 64] = core::array::from_fn(|i| a.process(buf[i]));
        b.run(&mut buf);
        assert_eq!(buf.map(f32::to_bits), want.map(f32::to_bits));
    }
}
