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

/// Approximate lut_stiffness: structure (0..1) → stiffness coefficient.
/// Rings: ranges from -0.0625 to +2.0 with a zero plateau at ~0.25.
fn stiffness_from_structure(structure: f32) -> f32 {
    if structure < 0.24 {
        // Negative stiffness (compressed partials, tube-like)
        -0.0625 * (1.0 - structure / 0.24)
    } else if structure < 0.3 {
        // Harmonic plateau (perfect string)
        0.0
    } else {
        // Stretch ramp: 0 to 0.5 (with stiffness decay in the loop,
        // this produces moderate to heavy inharmonicity without
        // pushing all modes past Nyquist)
        let t = (structure - 0.3) / 0.7;
        t * t * 0.5
    }
}

// ── Modal Params ────────────────────────────────────────────────────

/// Resonator model selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ResonatorMode {
    Modal = 0,
    String = 1,
    Bowed = 2,
}

impl ResonatorMode {
    pub fn from_u8(v: u8) -> Self {
        match v % 3 {
            0 => ResonatorMode::Modal,
            1 => ResonatorMode::String,
            _ => ResonatorMode::Bowed,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ModalParams {
    /// Resonator model: Modal / String / Bowed
    pub mode: u8, // 0-2
    pub excite: f32,
    pub decay: f32,
    pub brightness: f32,
    pub inharm: f32,
    pub position: f32,
    pub note: f32,
    pub num_modes: u8,
    // Per-mode params (page 2)
    /// String: dispersion (allpass detuning)
    pub dispersion: f32,
    /// Bowed: bow velocity
    pub bow_velocity: f32,
    /// Bowed: bow force/pressure
    pub bow_force: f32,
}

impl Default for ModalParams {
    fn default() -> Self {
        Self {
            mode: 0,
            excite: 0.8,
            decay: 0.5,
            brightness: 0.7,
            inharm: 0.25,
            position: 0.25,
            note: 60.0,
            num_modes: 32,
            dispersion: 0.0,
            bow_velocity: 0.5,
            bow_force: 0.5,
        }
    }
}

// ── Modal Engine ────────────────────────────────────────────────────

// ── Karplus-Strong delay line ────────────────────────────────────────

const MAX_DELAY: usize = 2048; // supports down to ~23Hz at 48kHz

struct KsString {
    buffer: [f32; MAX_DELAY],
    write_pos: usize,
    delay_len: usize,
    /// Fractional delay allpass coefficient
    frac_coeff: f32,
    frac_state: f32,
    /// Damping filter state (2-point average)
    damp_state: f32,
    /// Decay coefficient
    decay: f32,
}

impl KsString {
    fn new() -> Self {
        Self {
            buffer: [0.0; MAX_DELAY],
            write_pos: 0,
            delay_len: 100,
            frac_coeff: 0.0,
            frac_state: 0.0,
            damp_state: 0.0,
            decay: 0.999,
        }
    }

    fn set_freq(&mut self, freq: f32, sample_rate: u32, decay: f32, brightness: f32) {
        let period = sample_rate as f32 / freq;
        self.delay_len = (period as usize).min(MAX_DELAY - 1).max(2);
        let frac = period - self.delay_len as f32;
        // Allpass interpolation coefficient for fractional delay
        self.frac_coeff = (1.0 - frac) / (1.0 + frac);
        // Decay: longer delay = need higher coefficient to maintain same RT60
        self.decay = 0.995 + decay * 0.00499;
        self.decay *= 0.5 + brightness * 0.5; // brightness reduces damping
    }

    /// Fill the delay line with filtered noise (pluck excitation).
    fn pluck(&mut self, amplitude: f32, brightness: f32) {
        let mut noise_state = 0x87654321_u32;
        let mut lp = 0.0_f32;
        let cutoff = 0.2 + brightness * 0.7; // lowpass on the noise

        for i in 0..self.delay_len {
            noise_state ^= noise_state << 13;
            noise_state ^= noise_state >> 17;
            noise_state ^= noise_state << 5;
            let noise = (noise_state as i32) as f32 / i32::MAX as f32;
            lp += cutoff * (noise * amplitude - lp);
            self.buffer[i] = lp;
        }
        self.write_pos = self.delay_len; // so first read starts at buffer[0]
        self.damp_state = 0.0;
        self.frac_state = 0.0;
    }

    /// Process one sample.
    #[inline]
    fn tick(&mut self) -> f32 {
        // Read from delay line
        let read_pos = (self.write_pos + MAX_DELAY - self.delay_len) % MAX_DELAY;
        let sample = self.buffer[read_pos];

        // Fractional delay via allpass interpolation
        let allpass_out = self.frac_coeff * (sample - self.frac_state) + self.buffer[(read_pos + 1) % MAX_DELAY];
        self.frac_state = allpass_out;

        // Damping filter: 2-point average (Karplus-Strong classic)
        let damped = (allpass_out + self.damp_state) * 0.5 * self.decay;
        self.damp_state = allpass_out;

        // Write back
        self.buffer[self.write_pos] = damped;
        self.write_pos = (self.write_pos + 1) % MAX_DELAY;

        sample
    }
}

// ── Modal Engine (with String and Bowed modes) ──────────────────────

pub struct ModalEngine {
    filters: [Svf; MAX_MODES],
    cos_osc: CosineOsc,
    resolution: usize,
    // String model
    string: KsString,
    // Bowed model state
    bow_state: f32,
    // Shared
    frequency: f32, // normalized: Hz / sample_rate
    active_mode: ResonatorMode,
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
            string: KsString::new(),
            bow_state: 0.0,
            frequency: 220.0 / 48000.0,
            active_mode: ResonatorMode::Modal,
            exciter_remaining: 0,
            exciter_amp: 0.0,
            noise_state: 0x12345678,
            exciter_lp: 0.0,
            active: false,
            silence_counter: 0,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, params: &ModalParams, sample_rate: u32) {
        self.active_mode = ResonatorMode::from_u8(params.mode);
        let vel = velocity as f32 / 127.0;
        let freq = note_to_freq(note);
        self.frequency = freq / sample_rate as f32;

        match self.active_mode {
            ResonatorMode::Modal => {
                self.compute_filters(params, self.frequency);
                self.cos_osc.init(params.position);
                let burst_ms = 2.0 + params.excite * 4.0;
                self.exciter_remaining = (burst_ms * sample_rate as f32 / 1000.0) as usize;
                self.exciter_amp = vel * params.excite;
                self.exciter_lp = 0.0;
            }
            ResonatorMode::String => {
                self.string.set_freq(freq, sample_rate, params.decay, params.brightness);
                self.string.pluck(vel * params.excite, params.brightness);
            }
            ResonatorMode::Bowed => {
                // Bowed: set up string for continuous excitation
                self.string.set_freq(freq, sample_rate, params.decay, params.brightness);
                // Fill with silence — bow will drive it continuously
                for s in self.string.buffer.iter_mut() {
                    *s = 0.0;
                }
                self.bow_state = 0.0;
                self.exciter_amp = vel * params.bow_force;
            }
        }

        self.active = true;
        self.silence_counter = 0;
    }

    pub fn note_off(&mut self) {}

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Configure filters — called every render block (not just note_on).
    /// Matches Rings' ComputeFilters().
    fn compute_filters(&mut self, params: &ModalParams, frequency: f32) {
        let num = (params.num_modes as usize).min(MAX_MODES) & !1;
        self.resolution = num;

        // Q from decay. Higher Q = longer ring.
        let mut q = 200.0 + params.decay * params.decay * 1800.0; // 200..2000

        // Stiffness from structure/inharm
        let mut stiffness = stiffness_from_structure(params.inharm);

        // Brightness → q_loss per mode (Rings formula)
        let structure = params.inharm;
        let bright_atten = {
            let x = 1.0 - structure;
            let x2 = x * x;
            x2 * x2 * x2 * x2
        };
        let brightness = params.brightness * (1.0 - 0.2 * bright_atten);
        let mut q_loss = brightness * (2.0 - brightness) * 0.85 + 0.15;
        let q_loss_damping_rate = structure * (2.0 - structure) * 0.1;

        let mut harmonic = frequency;
        let mut stretch_factor = 1.0_f32;

        for i in 0..num {
            let partial_freq = (harmonic * stretch_factor).min(0.49);

            // Per-mode Q (Rings: 1.0 + partial_freq * q)
            let mode_q = 1.0 + partial_freq * q;
            self.filters[i].set(partial_freq, mode_q);

            // Accumulate stiffness with decay for negative values
            stretch_factor += stiffness;
            if stretch_factor < 0.1 { stretch_factor = 0.1; } // never go negative
            if stiffness < 0.0 { stiffness *= 0.93; } // decay negative stiffness

            // Q decays across modes (Rings: q *= q_loss)
            q *= q_loss;
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

        let mut max_level = 0.0_f32;

        // Recompute filters every block (Rings does this — allows live parameter changes)
        if self.active_mode == ResonatorMode::Modal {
            self.compute_filters(params, self.frequency);
            self.cos_osc.init(params.position);
        }

        match self.active_mode {
            ResonatorMode::Modal => self.render_modal(output, &mut max_level),
            ResonatorMode::String => self.render_string(output, &mut max_level),
            ResonatorMode::Bowed => self.render_bowed(output, params, &mut max_level),
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

    fn render_modal(&mut self, output: &mut [f32; BLOCK_SIZE], max_level: &mut f32) {
        let num = self.resolution;
        for s in output.iter_mut() {
            let excite = if self.exciter_remaining > 0 {
                self.exciter_remaining -= 1;
                let env = (self.exciter_remaining as f32 / 200.0).min(1.0);
                let raw = self.noise() * self.exciter_amp * env;
                self.exciter_lp += 0.4 * (raw - self.exciter_lp);
                self.exciter_lp
            } else {
                0.0
            };

            // Rings scales external audio input by 0.125. Our internal exciter
            // is already at the right level — no additional scaling needed.
            let input = excite;

            let mut odd = 0.0_f32;
            let mut even = 0.0_f32;
            self.cos_osc.start();

            let mut i = 0;
            while i + 1 < num {
                odd += self.cos_osc.next() * self.filters[i].process_bp(input);
                even += self.cos_osc.next() * self.filters[i + 1].process_bp(input);
                i += 2;
            }

            // Rings outputs odd and even separately (stereo). We sum to mono.
            // Soft-limit to prevent clipping at high Q.
            *s = libm::tanhf((odd + even) * 0.5) * 2.0;
            *max_level = max_level.max(libm::fabsf(*s));
        }
    }

    fn render_string(&mut self, output: &mut [f32; BLOCK_SIZE], max_level: &mut f32) {
        for s in output.iter_mut() {
            *s = self.string.tick();
            *max_level = max_level.max(libm::fabsf(*s));
        }
    }

    fn render_bowed(&mut self, output: &mut [f32; BLOCK_SIZE], params: &ModalParams, max_level: &mut f32) {
        let bow_vel = params.bow_velocity;
        let bow_force = self.exciter_amp;

        for s in output.iter_mut() {
            // Read current string velocity from delay line
            let read_pos = (self.string.write_pos + MAX_DELAY - self.string.delay_len) % MAX_DELAY;
            let string_vel = self.string.buffer[read_pos];

            // Bow interaction: friction model
            // velocity difference between bow and string
            let delta_v = bow_vel - string_vel;
            // Nonlinear friction (simplified bow table from Elements)
            let friction = bow_force * delta_v * libm::expf(-4.0 * delta_v * delta_v);

            // Inject friction force into the delay line
            let damped = (string_vel + friction + self.string.damp_state) * 0.5 * self.string.decay;
            self.string.damp_state = string_vel + friction;

            self.string.buffer[self.string.write_pos] = damped;
            self.string.write_pos = (self.string.write_pos + 1) % MAX_DELAY;

            *s = string_vel;
            *max_level = max_level.max(libm::fabsf(*s));
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
