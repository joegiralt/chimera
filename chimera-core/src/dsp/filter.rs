use crate::params::FilterParams;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FilterMode {
    Lp1 = 0,
    Lp2 = 1,
    Lp4 = 2,
    Bp2 = 3,
    Bp4 = 4,
    Hp4 = 5,
    Nt2 = 6,
    Phazor = 7,
}

impl FilterMode {
    pub fn from_u8(v: u8) -> Self {
        match v % 8 {
            0 => FilterMode::Lp1,
            1 => FilterMode::Lp2,
            2 => FilterMode::Lp4,
            3 => FilterMode::Bp2,
            4 => FilterMode::Bp4,
            5 => FilterMode::Hp4,
            6 => FilterMode::Nt2,
            _ => FilterMode::Phazor,
        }
    }
}

/// 2-pole state variable filter with nonlinear feedback.
/// The saturation is INSIDE the feedback loop — this is what gives
/// analog filters their character. At high resonance, the filter
/// self-oscillates with a warm, saturated tone.
#[derive(Clone, Debug)]
pub struct SvfFilter {
    ic1eq: [f32; 2],
    ic2eq: [f32; 2],
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
        }
    }

    pub fn process(&mut self, buf: &mut [f32], params: &FilterParams, sample_rate: u32) {
        let mode = FilterMode::from_u8(params.mode);
        let cutoff = params.cutoff.value;
        let reso = params.resonance.value;
        let drive = params.drive.value;

        let fc = cutoff.min(sample_rate as f32 * 0.49);
        let g = libm::tanf(core::f32::consts::PI * fc / sample_rate as f32);

        // Resonance: full range. k=2 (none) to k=0.01 (screaming self-osc).
        // Let it go all the way — the nonlinear feedback keeps it stable.
        let k = 2.0 * (1.0 - reso) + 0.01;

        for sample in buf.iter_mut() {
            let input = *sample * (1.0 + drive * 4.0);

            let out = match mode {
                FilterMode::Lp1 => self.tick_onepole(input, g, 0),
                FilterMode::Lp2 => {
                    let (lp, _, _) = self.tick_svf_nonlinear(input, g, k, 0);
                    lp
                }
                FilterMode::Lp4 => {
                    // Cascaded: lower resonance on second stage to prevent blowup
                    let (lp1, _, _) = self.tick_svf_nonlinear(input, g, k, 0);
                    let k2 = k * 1.2 + 0.3; // second stage less resonant
                    let (lp2, _, _) = self.tick_svf_nonlinear(lp1, g, k2, 1);
                    lp2
                }
                FilterMode::Bp2 => {
                    let (_, bp, _) = self.tick_svf_nonlinear(input, g, k, 0);
                    bp * 2.0 // boost BP output for presence
                }
                FilterMode::Bp4 => {
                    let (_, bp1, _) = self.tick_svf_nonlinear(input, g, k, 0);
                    let k2 = k * 1.2 + 0.3;
                    let (_, bp2, _) = self.tick_svf_nonlinear(bp1, g, k2, 1);
                    bp2 * 2.0
                }
                FilterMode::Hp4 => {
                    let (_, _, hp1) = self.tick_svf_nonlinear(input, g, k, 0);
                    let k2 = k * 1.2 + 0.3;
                    let (_, _, hp2) = self.tick_svf_nonlinear(hp1, g, k2, 1);
                    hp2
                }
                FilterMode::Nt2 => {
                    let (lp, _, hp) = self.tick_svf_nonlinear(input, g, k, 0);
                    lp + hp
                }
                FilterMode::Phazor => {
                    let (_, bp1, _) = self.tick_svf_nonlinear(input, g, k, 0);
                    let ap1 = input - 2.0 * k * bp1;
                    let (_, bp2, _) = self.tick_svf_nonlinear(ap1, g, k, 1);
                    ap1 - 2.0 * k * bp2
                }
            };

            *sample = out;
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
    fn tick_svf_nonlinear(
        &mut self,
        input: f32,
        g: f32,
        k: f32,
        stage: usize,
    ) -> (f32, f32, f32) {
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
