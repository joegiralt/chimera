//! What every string loop is built from, so none can run away (ADR 0056).

/// A string loop's gain per pass: always below 1.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct LoopGain(f32);

impl LoopGain {
    pub const MAX: f32 = 0.9995;
    /// The highest gain.
    pub const TOP: Self = Self(Self::MAX);

    /// `g` clamped to `[0, MAX]`; NaN is 0.
    pub fn new(g: f32) -> Self {
        // Not `clamp`, which passes NaN.
        Self(if g >= 0.0 { g.min(Self::MAX) } else { 0.0 })
    }

    /// The gain that falls 60 dB in `t60_s` at `freq_hz` passes a second.
    pub fn from_t60(t60_s: f32, freq_hz: f32) -> Self {
        // Not above 0: a negative T60 would ring longest.
        if t60_s > 0.0 {
            Self::new(libm::powf(0.001, 1.0 / (t60_s * freq_hz)))
        } else {
            Self(0.0)
        }
    }

    pub fn get(self) -> f32 {
        self.0
    }

    pub fn min(self, o: Self) -> Self {
        Self(self.0.min(o.0))
    }
}

/// A note-off's ramp, in samples: 5 ms.
pub const RELEASE_SAMPLES: u32 = 240;
/// Seconds, a released string's ring.
pub const RELEASE_T60: f32 = 0.12;

/// A note-off's ramp from the held gain to the released one, a sample at a
/// time. Unstarted, it passes the held gain.
#[derive(Clone, Copy, Debug)]
pub struct Release {
    left: u32,
    from: f32,
    to: f32,
}

impl Release {
    /// Not released: `gain` is the held gain.
    pub const HELD: Self = Self {
        left: 0,
        from: LoopGain::MAX,
        to: LoopGain::MAX,
    };

    /// Ramps from `from` to `to`, never up: a second note-off ramps on
    /// from where the first has got to.
    pub fn start(&mut self, from: LoopGain, to: LoopGain) {
        let from = from.min(LoopGain(self.now()));
        *self = Self {
            left: RELEASE_SAMPLES,
            from: from.get(),
            to: to.min(from).get(),
        };
    }

    /// The ramp's gain now: `to` past it.
    #[inline]
    fn now(&self) -> f32 {
        let t = 1.0 - self.left as f32 / RELEASE_SAMPLES as f32;
        self.from + (self.to - self.from) * t
    }

    /// This sample's gain. Past the ramp, `held` no higher than `to`: the
    /// release never gives the gain back, whatever DAMP does.
    #[inline]
    pub fn gain(&mut self, held: LoopGain) -> LoopGain {
        if self.idle() {
            return held.min(LoopGain(self.to));
        }
        let g = self.now();
        self.left -= 1;
        LoopGain(g)
    }

    /// Not ramping.
    pub fn idle(&self) -> bool {
        self.left == 0
    }
}

/// The DC blocker's corner. It sits on a string model's output, not in the
/// loop, where its phase would detune the upper partials (ADR 0056).
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

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

/// The shortest line the loop's three-tap low-pass reads.
pub const MIN_LINE: usize = 2;

/// A first-order allpass, `(η + z⁻¹)/(1 + η z⁻¹)`: the loop's fraction of
/// a sample. Stable for `|η| < 1`.
#[derive(Clone, Copy, Default)]
pub struct Allpass1 {
    eta: f32,
    x1: f32,
    y1: f32,
}

impl Allpass1 {
    pub fn set(&mut self, eta: f32) {
        self.eta = eta;
    }

    #[cfg(test)]
    pub fn eta(&self) -> f32 {
        self.eta
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.eta * (x - self.y1) + self.x1;
        self.x1 = x;
        self.y1 = y;
        y
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

/// `Allpass1`'s phase delay at `w` rad/sample, in samples.
pub fn allpass_phase_delay(eta: f32, w: f32) -> f32 {
    1.0 - 2.0 * libm::atan2f(eta * libm::sinf(w), 1.0 + eta * libm::cosf(w)) / w
}

/// The η whose phase delay at `w` is `frac`, exactly; `w` held to
/// `[1e-6, 3]`, so 0 Hz is not NaN.
pub fn eta_for(frac: f32, w: f32) -> f32 {
    let w = w.clamp(1e-6, 3.0);
    let theta = w * (1.0 - frac) * 0.5;
    libm::sinf(theta) / libm::sinf(w - theta)
}

/// A loop of `period` samples whose other parts delay `other` at `w`: the
/// line delay and the allpass's η. The fraction is in `[0.5, 1.5)` unless
/// the line clamps to `MIN_LINE`.
pub fn split(period: f32, other: f32, w: f32) -> (usize, f32) {
    let d = period - other;
    let n = (libm::floorf(d - 0.5) as usize).max(MIN_LINE);
    (n, eta_for(d - n as f32, w))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;
    fn w_of(hz: f32) -> f32 {
        core::f32::consts::TAU * hz / SR
    }

    #[test]
    fn eta_for_inverts_the_phase_delay() {
        for w in [w_of(49.0), w_of(2093.0)] {
            for frac in [0.5, 0.75, 1.0, 1.49] {
                let got = allpass_phase_delay(eta_for(frac, w), w);
                assert!((got - frac).abs() < 1e-4, "ω {w}, {frac}: {got}");
            }
        }
    }

    #[test]
    fn eta_for_is_finite_at_the_ends() {
        for w in [0.0, 1e-9, 3.0, 4.0] {
            for frac in [0.5, 1.0, 1.5] {
                assert!(eta_for(frac, w).is_finite(), "ω {w}, {frac}");
            }
        }
    }

    #[test]
    fn split_keeps_the_fraction_in_range() {
        let w = w_of(SR / 979.59);
        assert_eq!(split(979.59, 0.0, w).0, 979);
        let mut period = 22.9_f32;
        while period <= 979.6 {
            let w = w_of(SR / period);
            let (n, eta) = split(period, 0.0, w);
            let frac = allpass_phase_delay(eta, w);
            assert!((0.5 - 1e-3..1.5).contains(&frac), "{period}: {frac}");
            assert!((n as f32 + frac - period).abs() < 1e-3, "{period}");
            period *= 1.0007;
        }
    }

    #[test]
    fn loop_gain_from_a_negative_t60_is_the_shortest() {
        assert_eq!(LoopGain::from_t60(-1.0, 49.0).get(), 0.0);
    }

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
    fn release_ramps_then_never_gives_the_gain_back() {
        let (held, to) = (LoopGain::new(0.999), LoopGain::new(0.9));
        let mut r = Release::HELD;
        assert!(r.idle());
        assert_eq!(r.gain(held), held);
        r.start(held, to);
        let ramp: [f32; RELEASE_SAMPLES as usize] = core::array::from_fn(|_| r.gain(held).get());
        assert_eq!(ramp[0], held.get());
        assert!(ramp.windows(2).all(|w| w[1] < w[0]), "falls every sample");
        let step = (held.get() - to.get()) / RELEASE_SAMPLES as f32;
        assert!((ramp[RELEASE_SAMPLES as usize - 1] - (to.get() + step)).abs() < 1e-6);
        assert!(r.idle());
        // DAMP at its top: still `to`. Below it: the held gain.
        assert_eq!(r.gain(LoopGain::new(1.0)), to);
        assert_eq!(r.gain(LoopGain::new(0.5)).get(), 0.5);
    }

    #[test]
    fn a_second_note_off_never_gives_the_gain_back() {
        let (held, to) = (LoopGain::new(0.999), LoopGain::new(0.9));
        let mut r = Release::HELD;
        r.start(held, to);
        for _ in 0..RELEASE_SAMPLES / 2 {
            r.gain(held);
        }
        let mid = r.gain(held).get();
        r.start(held, to);
        assert!(r.gain(held).get() <= mid);
        for _ in 0..RELEASE_SAMPLES {
            r.gain(held);
        }
        r.start(held, to);
        assert_eq!(r.gain(held), to);
    }

    #[test]
    fn a_release_never_ramps_up() {
        let mut r = Release::HELD;
        r.start(LoopGain::new(0.9), LoopGain::new(0.99));
        for _ in 0..=RELEASE_SAMPLES {
            assert_eq!(r.gain(LoopGain::new(0.9)).get(), 0.9);
        }
    }

    #[test]
    fn dc_blocker_gain_is_at_most_one() {
        let dc = DcBlocker::new(48_000);
        let (r, g) = (dc.r as f64, dc.g as f64);
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
