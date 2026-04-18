use crate::params::FilterParams;

/// Filter mode selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FilterMode {
    Lp1 = 0,  // 1-pole lowpass (6dB/oct)
    Lp2 = 1,  // 2-pole lowpass (12dB/oct)
    Lp4 = 2,  // 4-pole lowpass (24dB/oct)
    Bp2 = 3,  // 2-pole bandpass
    Bp4 = 4,  // 4-pole bandpass
    Hp4 = 5,  // 4-pole highpass
    Nt2 = 6,  // 2-pole notch
    Phazor = 7, // allpass cascade (phaser)
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

/// 2-pole state variable filter (SVF).
/// Cascaded for 4-pole modes. Topology-preserving integrator design
/// for stable self-oscillation at high resonance.
#[derive(Clone, Debug)]
pub struct SvfFilter {
    // Two cascaded SVF stages for 4-pole modes
    ic1eq: [f32; 2], // integrator state 1 (per stage)
    ic2eq: [f32; 2], // integrator state 2 (per stage)
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

    /// Process a block of samples in-place.
    pub fn process(&mut self, buf: &mut [f32], params: &FilterParams, sample_rate: u32) {
        let mode = FilterMode::from_u8(params.mode);
        let cutoff = params.cutoff.value;
        let reso = params.resonance.value;
        let drive = params.drive.value;

        // SVF coefficient: g = tan(π * fc / fs)
        // Clamp cutoff to avoid instability near Nyquist
        let fc = cutoff.min(sample_rate as f32 * 0.49);
        let g = libm::tanf(core::f32::consts::PI * fc / sample_rate as f32);
        // Damping factor: k=2 (no resonance) to k~0 (self-oscillation).
        // For 4-pole cascaded modes, limit resonance to prevent runaway.
        let reso_clamped = match mode {
            FilterMode::Lp4 | FilterMode::Bp4 | FilterMode::Hp4 | FilterMode::Phazor => {
                reso * 0.8 // 4-pole: cap effective resonance
            }
            _ => reso,
        };
        let k = 2.0 - 2.0 * reso_clamped;

        for sample in buf.iter_mut() {
            // Input drive
            let input = *sample * (1.0 + drive * 3.0);

            let (out, _) = match mode {
                FilterMode::Lp1 => {
                    let lp = self.tick_onepole(input, g, 0);
                    (lp, 0.0)
                }
                FilterMode::Lp2 => {
                    let (lp, _, _) = self.tick_svf(input, g, k, 0);
                    (lp, 0.0)
                }
                FilterMode::Lp4 => {
                    let (lp1, _, _) = self.tick_svf(input, g, k, 0);
                    let (lp2, _, _) = self.tick_svf(lp1, g, k, 1);
                    (lp2, 0.0)
                }
                FilterMode::Bp2 => {
                    let (_, bp, _) = self.tick_svf(input, g, k, 0);
                    (bp, 0.0)
                }
                FilterMode::Bp4 => {
                    let (_, bp1, _) = self.tick_svf(input, g, k, 0);
                    let (_, bp2, _) = self.tick_svf(bp1, g, k, 1);
                    (bp2, 0.0)
                }
                FilterMode::Hp4 => {
                    let (lp1, _, hp1) = self.tick_svf(input, g, k, 0);
                    let (_, _, hp2) = self.tick_svf(hp1, g, k, 1);
                    let _ = lp1;
                    (hp2, 0.0)
                }
                FilterMode::Nt2 => {
                    let (lp, _, hp) = self.tick_svf(input, g, k, 0);
                    (lp + hp, 0.0) // notch = LP + HP
                }
                FilterMode::Phazor => {
                    // Allpass: 2*BP - input (from SVF identity)
                    let (_, bp1, _) = self.tick_svf(input, g, k, 0);
                    let ap1 = input - 2.0 * k * bp1;
                    let (_, bp2, _) = self.tick_svf(ap1, g, k, 1);
                    let ap2 = ap1 - 2.0 * k * bp2;
                    (ap2, 0.0)
                }
            };

            // Soft-limit output to prevent runaway at extreme resonance
            *sample = libm::tanhf(out);
        }
    }

    /// One-pole lowpass (6dB/oct).
    fn tick_onepole(&mut self, input: f32, g: f32, stage: usize) -> f32 {
        let v = (input - self.ic1eq[stage]) * g / (1.0 + g);
        let lp = v + self.ic1eq[stage];
        self.ic1eq[stage] = lp + v;
        lp
    }

    /// Topology-preserving SVF tick. Returns (lowpass, bandpass, highpass).
    fn tick_svf(&mut self, input: f32, g: f32, k: f32, stage: usize) -> (f32, f32, f32) {
        let v1 = self.ic1eq[stage];
        let v2 = self.ic2eq[stage];

        let hp = (input - (k + g) * v1 - v2) / (1.0 + k * g + g * g);
        let bp = g * hp + v1;
        let lp = g * bp + v2;

        self.ic1eq[stage] = bp + g * hp;
        self.ic2eq[stage] = lp + g * bp;

        (lp, bp, hp)
    }
}
