use crate::params::FilterParams;

/// The SVF's modes. The discriminants are the old `mode` byte (#111), so
/// every Sound keeps its mode (LP24 = 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FilterMode {
    Lp6 = 0,
    Lp12 = 1,
    Lp24 = 2,
    Bp12 = 3,
    Bp24 = 4,
    Hp24 = 5,
    Notch = 6,
    Phaser = 7,
}

impl FilterMode {
    /// In discriminant order.
    pub const ALL: [FilterMode; 8] = [
        FilterMode::Lp6,
        FilterMode::Lp12,
        FilterMode::Lp24,
        FilterMode::Bp12,
        FilterMode::Bp24,
        FilterMode::Hp24,
        FilterMode::Notch,
        FilterMode::Phaser,
    ];

    pub const fn label(self) -> &'static str {
        SVF_MODE_NAMES_BY_ID[self as usize]
    }
}

/// The SVF's modes as its MODE knob steps them, the default first (spec § 7).
pub const SVF_MODES: [FilterMode; 8] = [
    FilterMode::Lp24,
    FilterMode::Lp6,
    FilterMode::Lp12,
    FilterMode::Bp12,
    FilterMode::Bp24,
    FilterMode::Hp24,
    FilterMode::Notch,
    FilterMode::Phaser,
];

/// `SVF_MODES`' names, for MODE's spec.
pub static SVF_MODE_NAMES: [&str; 8] = [
    "LP24", "LP6", "LP12", "BP12", "BP24", "HP24", "NOTCH", "PHASER",
];

const SVF_MODE_NAMES_BY_ID: [&str; 8] = [
    "LP6", "LP12", "LP24", "BP12", "BP24", "HP24", "NOTCH", "PHASER",
];

/// Octaves a full CUTOFF route moves (spec § 3).
pub const CUTOFF_OCTAVES: f32 = 10.0;

/// The cutoff a route sum `sum` gives from `base` (spec § 3): Σ = 0 is
/// `base` bit for bit; the filter clamps to 0.49·fs as well.
pub fn routed_cutoff(base: f32, sum: f32) -> f32 {
    if sum == 0.0 {
        return base;
    }
    crate::params::FILTER_SPECS[0].offset(base, sum)
}

/// Sample `i`'s coefficient on a ramp from `from` by `step` a sample: the
/// block's last sample lands on the new `g` (#53).
#[inline(always)]
pub fn g_at(from: f32, step: f32, i: usize) -> f32 {
    from + step * (i + 1) as f32
}

/// 2-pole state variable filter with nonlinear feedback.
/// The saturation is INSIDE the feedback loop — this is what gives
/// analog filters their character. At high resonance, the filter
/// self-oscillates with a warm, saturated tone.
#[derive(Clone, Debug)]
pub struct SvfFilter {
    ic1eq: [f32; 2],
    ic2eq: [f32; 2],
    /// The last block's `g`; `None` on a fresh voice, which starts unramped.
    g: Option<f32>,
}

impl Default for SvfFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl SvfFilter {
    pub fn new() -> Self {
        Self {
            ic1eq: [0.0; 2],
            ic2eq: [0.0; 2],
            g: None,
        }
    }

    pub fn process(&mut self, buf: &mut [f32], params: &FilterParams, sample_rate: u32) {
        let mode = params.mode();
        let drive = params.drive;
        let fc = params.cutoff.min(sample_rate as f32 * 0.49);
        let g = crate::dsp::fast_tan(core::f32::consts::PI * fc / sample_rate as f32);
        let from = self.g.replace(g).unwrap_or(g);

        // Resonance: full range. k=2 (none) to k=0.01 (screaming self-osc).
        // Let it go all the way — the nonlinear feedback keeps it stable.
        let k = 2.0 * (1.0 - params.resonance) + 0.01;

        if from == g {
            for sample in buf.iter_mut() {
                *sample = self.tick(mode, *sample * (1.0 + drive * 4.0), g, k);
            }
        } else {
            // #53: `g` ramps to this block's value, reached on the last sample.
            let step = (g - from) / buf.len() as f32;
            for (i, sample) in buf.iter_mut().enumerate() {
                let gi = g_at(from, step, i);
                *sample = self.tick(mode, *sample * (1.0 + drive * 4.0), gi, k);
            }
        }
    }

    /// Forget the last `g`: the next block starts without a ramp.
    pub fn hold(&mut self) {
        self.g = None;
    }

    /// The last block's `g` (`None`: no ramp next block). Test-only: nothing
    /// in the audio path needs to read the ramp state back.
    #[cfg(test)]
    pub(crate) fn last_g(&self) -> Option<f32> {
        self.g
    }

    fn tick(&mut self, mode: FilterMode, input: f32, g: f32, k: f32) -> f32 {
        match mode {
            FilterMode::Lp6 => self.tick_onepole(input, g, 0),
            FilterMode::Lp12 => {
                let (lp, _, _) = self.tick_svf_nonlinear(input, g, k, 0);
                lp
            }
            FilterMode::Lp24 => {
                // Cascaded: lower resonance on second stage to prevent blowup
                let (lp1, _, _) = self.tick_svf_nonlinear(input, g, k, 0);
                let k2 = k * 1.2 + 0.3; // second stage less resonant
                let (lp2, _, _) = self.tick_svf_nonlinear(lp1, g, k2, 1);
                lp2
            }
            FilterMode::Bp12 => {
                let (_, bp, _) = self.tick_svf_nonlinear(input, g, k, 0);
                bp * 2.0 // boost BP output for presence
            }
            FilterMode::Bp24 => {
                let (_, bp1, _) = self.tick_svf_nonlinear(input, g, k, 0);
                let k2 = k * 1.2 + 0.3;
                let (_, bp2, _) = self.tick_svf_nonlinear(bp1, g, k2, 1);
                bp2 * 2.0
            }
            FilterMode::Hp24 => {
                let (_, _, hp1) = self.tick_svf_nonlinear(input, g, k, 0);
                let k2 = k * 1.2 + 0.3;
                let (_, _, hp2) = self.tick_svf_nonlinear(hp1, g, k2, 1);
                hp2
            }
            FilterMode::Notch => {
                let (lp, _, hp) = self.tick_svf_nonlinear(input, g, k, 0);
                lp + hp
            }
            FilterMode::Phaser => {
                let (_, bp1, _) = self.tick_svf_nonlinear(input, g, k, 0);
                let ap1 = input - 2.0 * k * bp1;
                let (_, bp2, _) = self.tick_svf_nonlinear(ap1, g, k, 1);
                ap1 - 2.0 * k * bp2
            }
        }
    }

    fn tick_onepole(&mut self, input: f32, g: f32, stage: usize) -> f32 {
        let v = (input - self.ic1eq[stage]) * g / (1.0 + g);
        let lp = v + self.ic1eq[stage];
        self.ic1eq[stage] = lp + v;
        lp
    }

    /// SVF with nonlinear saturation in the feedback path.
    /// The tanh inside the loop is what gives it analog character —
    /// resonance builds up but saturates naturally instead of exploding.
    fn tick_svf_nonlinear(&mut self, input: f32, g: f32, k: f32, stage: usize) -> (f32, f32, f32) {
        // Saturate the integrator states — this is the "analog" part.
        // The nonlinearity inside the loop means the filter self-limits
        // at high resonance instead of blowing up. It also creates
        // subtle harmonic distortion that varies with signal level.
        let v1 = saturate(self.ic1eq[stage]);
        let v2 = saturate(self.ic2eq[stage]);

        let hp = (input - (k + g) * v1 - v2) / (1.0 + k * g + g * g);
        let bp = g * hp + v1;
        let lp = g * bp + v2;

        self.ic1eq[stage] = bp + g * hp;
        self.ic2eq[stage] = lp + g * bp;

        (lp, bp, hp)
    }
}

/// Soft saturation — gentle curve that limits amplitude while preserving
/// small signals. This is milder than tanh, letting the resonance peak
/// ring out before clamping. Sounds more like analog capacitor saturation.
#[inline]
fn saturate(x: f32) -> f32 {
    // Cubic soft clip: linear for |x| < 1, soft limit beyond
    if x > 1.5 {
        1.0
    } else if x < -1.5 {
        -1.0
    } else if x > 1.0 {
        1.0 - (2.0 - x) * (2.0 - x) / 6.0
    } else if x < -1.0 {
        -1.0 + (2.0 + x) * (2.0 + x) / 6.0
    } else {
        x
    }
}
