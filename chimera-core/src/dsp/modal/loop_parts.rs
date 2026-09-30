//! What every string loop is built from, so none can run away (ADR 0056).

/// A string loop's gain per pass: always below 1.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct LoopGain(f32);

impl LoopGain {
    /// A T60 of 690,000 / f0 s: past DAMP's 20 s to 34 kHz. Once 0.9995,
    /// which held C6 to 13 s and G6 to 9.
    pub const MAX: f32 = 0.99999;
    /// The highest gain.
    #[cfg(test)]
    pub const TOP: Self = Self(Self::MAX);

    /// `g` clamped to `[0, MAX]`; NaN is 0.
    pub fn new(g: f32) -> Self {
        // Not `clamp`, which passes NaN.
        Self(if g >= 0.0 { g.min(Self::MAX) } else { 0.0 })
    }

    /// The gain that falls 60 dB in `t60_s` at `freq_hz` passes a second.
    pub fn from_t60(t60_s: f32, freq_hz: f32) -> Self {
        Self::new(fall(t60_s, freq_hz))
    }

    pub fn get(self) -> f32 {
        self.0
    }

    pub fn min(self, o: Self) -> Self {
        Self(self.0.min(o.0))
    }
}

/// The share of DAMP's loss a pass the loop low-pass may take at f0.
/// Below 1, so the loop's gain at 0 Hz, its highest, stays below 1.
pub const LP_SHARE: f32 = 0.5;

/// `0.001^(1 / (t60_s·freq_hz))`, as `expf`, cheaper than `powf`: the
/// gain a pass that falls 60 dB in `t60_s`. Not above 0 is 0: a negative
/// T60 would ring longest.
#[inline]
fn fall(t60_s: f32, freq_hz: f32) -> f32 {
    const LN_1000: f32 = 6.907_755;
    if t60_s > 0.0 {
        libm::expf(-LN_1000 / (t60_s * freq_hz))
    } else {
        0.0
    }
}

/// `1 − cos w`, its series to `w⁶`: within 0.3 % to `w` = 1.8, a loop of
/// 3.5 samples.
#[inline]
fn one_less_cos(w: f32) -> f32 {
    let x = w * w;
    0.5 * x * (1.0 - x / 12.0 * (1.0 - x / 30.0))
}

/// A string loop's low-pass side taps and gain at `freq_hz` (`w`
/// rad/sample) for a `t60_s` ring: the low-pass's loss at f0,
/// `lp·(1 − cos w)`, is made up in the gain, so the fundamental rings
/// `t60_s` at every pitch. The low-pass takes at most `LP_SHARE` of the
/// ring's loss a pass: a long ring on a high note needs a brighter loop.
/// Every other frequency's gain is `gain·(1 − lp·(1 − cos ω))`, highest
/// at 0 Hz, under `g / (1 − LP_SHARE·(1 − g))` for the ring's `g`, so
/// below one. The make-up, `1 / (1 − loss)`, is its series to `loss⁴`, a
/// hair under it (`loss` is under 0.01 to C7): no divide, and one `expf`
/// for `powf`, so the block costs no more than before.
pub fn damped(t60_s: f32, (freq_hz, w): (f32, f32), lp: f32) -> (f32, LoopGain) {
    let g = fall(t60_s, freq_hz);
    let unit = one_less_cos(w);
    let room = LP_SHARE * (1.0 - g);
    // A divide only where the share binds.
    let lp = if lp * unit > room { room / unit } else { lp };
    let l = lp * unit;
    let make_up = 1.0 + l * (1.0 + l * (1.0 + l * (1.0 + l)));
    (lp, LoopGain::new(g * make_up))
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

    /// Ramps from `from` to `to`, up or down: a lifted bow's loop, linear
    /// once the bow is off, rings on DAMP's gain whatever the bowed loss.
    pub fn lift(&mut self, from: LoopGain, to: LoopGain) {
        *self = Self {
            left: RELEASE_SAMPLES,
            from: from.get(),
            to: to.get(),
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

    /// Samples of ramp left.
    pub fn left(&self) -> usize {
        self.left as usize
    }
}

/// The DC blocker's corner. It sits on a string model's output, not in the
/// loop, where its phase would detune the upper partials (ADR 0056).
pub const DC_HZ: f32 = 10.0;

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

    /// The fundamental's gain is the ring's; no frequency's passes 1.
    #[test]
    fn damped_rings_the_fundamental_and_stays_below_one() {
        for hz in [49.0, 261.6, 1046.5, 2093.0, 8000.0] {
            let w = w_of(hz);
            for t60 in [0.05, 1.0, 20.0] {
                for lp in [0.0475, 0.25] {
                    let (c, g) = damped(t60, (hz, w), lp);
                    assert!(c <= lp && c >= 0.0, "{hz} {t60} {lp}: {c}");
                    let at = |w: f32| g.get() * (1.0 - c * (1.0 - libm::cosf(w)));
                    let want = LoopGain::from_t60(t60, hz).get();
                    // The make-up's series: a hair under, never over.
                    let tol = 2e-6 + want * libm::powf(0.5 * (1.0 - want), 5.0);
                    assert!(
                        (at(w) - want).abs() < tol,
                        "{hz} {t60} {lp}: {} {want}",
                        at(w)
                    );
                    assert!(at(0.0) < 1.0, "{hz} {t60} {lp}: {}", at(0.0));
                }
            }
        }
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
}
