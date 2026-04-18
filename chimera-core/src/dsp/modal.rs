use chimera_hal::BLOCK_SIZE;

const MAX_MODES: usize = 48;

// ── SVF Bandpass (ZDF topology, matching Rings/stmlib) ──────────────

#[derive(Clone, Debug)]
struct Svf {
    state_1: f32,
    state_2: f32,
    g: f32, // tan(pi * f)
    r: f32, // 1/Q
    h: f32, // 1 / (1 + r*g + g*g)
}

impl Svf {
    fn new() -> Self {
        Self { state_1: 0.0, state_2: 0.0, g: 0.0, r: 1.0, h: 1.0 }
    }

    /// Configure filter. `freq` = normalized frequency (Hz/sr), `resonance` = Q.
    fn set(&mut self, freq: f32, resonance: f32) {
        self.g = tan_approx(freq);
        self.r = 1.0 / resonance.max(0.5);
        self.h = 1.0 / (1.0 + self.r * self.g + self.g * self.g);
    }

    /// Process one sample, return bandpass output.
    #[inline]
    fn process_bp(&mut self, input: f32) -> f32 {
        let hp = (input - self.r * self.state_1 - self.g * self.state_1 - self.state_2) * self.h;
        let bp = self.g * hp + self.state_1;
        self.state_1 = self.g * hp + bp;
        let lp = self.g * bp + self.state_2;
        self.state_2 = self.g * bp + lp;
        bp
    }
}

/// Fast tangent approximation (matches Rings' FREQUENCY_FAST).
fn tan_approx(f: f32) -> f32 {
    let pi = core::f32::consts::PI;
    let f2 = f * f;
    f * (pi + f2 * (0.326 * pi * pi * pi + 0.1823 * pi * pi * pi * pi * pi * f2))
}

// ── Cosine Oscillator (position weighting, matching Rings) ──────────

struct CosineOsc {
    y0: f32,
    y1: f32,
    iir_coefficient: f32,
    initial_amplitude: f32,
}

impl CosineOsc {
    fn new() -> Self {
        Self { y0: 0.0, y1: 0.0, iir_coefficient: 0.0, initial_amplitude: 0.0 }
    }

    /// Initialize with position (0..1).
    fn init(&mut self, position: f32) {
        let mut sign = 16.0_f32;
        let mut freq = position - 0.25;
        if freq < 0.0 {
            freq = -freq;
        } else if freq > 0.5 {
            freq -= 0.5;
        } else {
            sign = -16.0;
        }
        self.iir_coefficient = sign * freq * (1.0 - 2.0 * freq);
        self.initial_amplitude = self.iir_coefficient * 0.25;
    }

    /// Reset to start of sequence.
    fn start(&mut self) {
        self.y1 = self.initial_amplitude;
        self.y0 = 0.5;
    }

    /// Get next amplitude weight (call once per mode).
    #[inline]
    fn next(&mut self) -> f32 {
        let temp = self.y0;
        self.y0 = self.iir_coefficient * self.y0 - self.y1;
        self.y1 = temp;
        temp + 0.5 // shift to [0, 1]
    }
}

// ── Stiffness lookup table ──────────────────────────────────────────
// Maps structure (0..1) to stiffness coefficient.
// 0.0 → -0.0625 (compressed, tube-like)
// ~0.25 → 0.0 (perfect harmonics)
// 1.0 → 2.0 (very stretched, metallic)

fn stiffness_from_structure(structure: f32) -> f32 {
    if structure < 0.24 {
        -0.02 * (1.0 - structure / 0.24)
    } else if structure < 0.3 {
        0.0 // harmonic plateau
    } else {
        let t = (structure - 0.3) / 0.7;
        t * t * 0.15 // max stiffness 0.15 — enough for bell-like spread
    }
}

// ── Modal Params ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct ModalParams {
    pub excite: f32,
    pub decay: f32,       // 0..1 → maps to Q
    pub damping: f32,     // 0..1 (unused name kept for UI compat — this is "structure")
    pub note: f32,
    pub brightness: f32,
    pub position: f32,
    pub inharm: f32,      // 0..1 → structure (stiffness)
    pub num_modes: u8,
}

impl Default for ModalParams {
    fn default() -> Self {
        Self {
            excite: 0.8,
            decay: 0.5,
            damping: 0.3,
            note: 60.0,
            brightness: 0.7,
            position: 0.25,
            inharm: 0.25, // harmonic by default (maps to stiffness ≈ 0)
            num_modes: 32,
        }
    }
}

// ── Modal Engine ────────────────────────────────────────────────────

pub struct ModalEngine {
    filters: [Svf; MAX_MODES],
    cos_osc: CosineOsc,
    resolution: usize,
    exciter_remaining: usize,
    exciter_amp: f32,
    noise_state: u32,
    exciter_lp: f32,
    active: bool,
    silence_counter: u32,
}

impl Default for ModalEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ModalEngine {
    pub fn new() -> Self {
        Self {
            filters: core::array::from_fn(|_| Svf::new()),
            cos_osc: CosineOsc::new(),
            resolution: 0,
            exciter_remaining: 0,
            exciter_amp: 0.0,
            noise_state: 0x12345678,
            exciter_lp: 0.0,
            active: false,
            silence_counter: 0,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, params: &ModalParams, sample_rate: u32) {
        self.compute_filters(note, params, sample_rate);
        self.cos_osc.init(params.position);

        let burst_ms = 2.0 + params.excite * 4.0;
        self.exciter_remaining = (burst_ms * sample_rate as f32 / 1000.0) as usize;
        self.exciter_amp = velocity as f32 / 127.0 * params.excite;
        self.exciter_lp = 0.0;
        self.active = true;
        self.silence_counter = 0;
    }

    pub fn note_off(&mut self) {}

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Configure filters (called once per note, matching Rings' ComputeFilters).
    fn compute_filters(&mut self, note: u8, params: &ModalParams, sample_rate: u32) {
        let frequency = note_to_freq(note) / sample_rate as f32; // normalized
        let num = (params.num_modes as usize).min(MAX_MODES);
        // Force even for odd/even splitting
        let num = num & !1;
        self.resolution = num;

        // Q from damping (Rings: 500 * lut_4_decades[damping])
        // Map decay 0..1 logarithmically: Q from 50 to 50000
        let q = 50.0 * libm::powf(10.0, params.decay * 3.0);

        // Stiffness from structure/inharm
        let mut stiffness = stiffness_from_structure(params.inharm);

        // Brightness → q_loss per mode
        let structure = params.inharm;
        let bright_atten = {
            let x = 1.0 - structure;
            let x2 = x * x;
            x2 * x2 * x2 * x2 // (1-structure)^8
        };
        let brightness = params.brightness * (1.0 - 0.2 * bright_atten);
        let mut q_loss = brightness * (2.0 - brightness) * 0.85 + 0.15;
        let q_loss_damping_rate = structure * (2.0 - structure) * 0.1;

        let mut harmonic = frequency;
        let mut stretch_factor = 1.0_f32;

        for i in 0..num {
            let partial_freq = (harmonic * stretch_factor).min(0.49);

            // Per-mode Q: increases with frequency (higher partials ring longer)
            let mode_q = 1.0 + partial_freq * q;
            self.filters[i].set(partial_freq, mode_q * q_loss);

            // Accumulate stiffness
            stretch_factor += stiffness;
            if stiffness < 0.0 {
                stiffness *= 0.93;
            } else {
                stiffness *= 0.98;
            }

            // Q loss for next mode
            q_loss += q_loss_damping_rate * (1.0 - q_loss);

            harmonic += frequency;
        }
    }

    pub fn render(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ModalParams,
        _sample_rate: u32,
    ) {
        if !self.active {
            for s in output.iter_mut() {
                *s = 0.0;
            }
            return;
        }

        let num = self.resolution;
        let mut max_level = 0.0_f32;

        for s in output.iter_mut() {
            // Exciter: shaped noise burst
            let excite = if self.exciter_remaining > 0 {
                self.exciter_remaining -= 1;
                let env = (self.exciter_remaining as f32 / 200.0).min(1.0);
                let raw = self.noise() * self.exciter_amp * env;
                self.exciter_lp += 0.4 * (raw - self.exciter_lp);
                self.exciter_lp
            } else {
                0.0
            };

            // Input scaling — Rings uses 0.125 but with a full-range audio input.
            // Our exciter is weaker, so scale up.
            let input = excite;

            // Process all modes in pairs: odd modes → out, even modes → aux
            // (We sum both to mono for now)
            let mut odd = 0.0_f32;
            let mut even = 0.0_f32;

            self.cos_osc.start();

            let mut i = 0;
            while i + 1 < num {
                let amp_odd = self.cos_osc.next();
                odd += amp_odd * self.filters[i].process_bp(input);
                let amp_even = self.cos_osc.next();
                even += amp_even * self.filters[i + 1].process_bp(input);
                i += 2;
            }

            *s = (odd + even) * 0.25;
            max_level = max_level.max(libm::fabsf(*s));
        }

        if max_level < 0.0001 && self.exciter_remaining == 0 {
            self.silence_counter += 1;
            if self.silence_counter > 10 {
                self.active = false;
            }
        } else {
            self.silence_counter = 0;
        }
    }

    #[inline]
    fn noise(&mut self) -> f32 {
        self.noise_state ^= self.noise_state << 13;
        self.noise_state ^= self.noise_state >> 17;
        self.noise_state ^= self.noise_state << 5;
        (self.noise_state as i32) as f32 / i32::MAX as f32
    }
}

fn note_to_freq(note: u8) -> f32 {
    440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0)
}
