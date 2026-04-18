use chimera_hal::BLOCK_SIZE;

/// Maximum number of resonant modes.
const MAX_MODES: usize = 24;

/// A single resonant mode — 2nd order bandpass (complex resonator).
/// Uses the efficient form: y[n] = 2*R*cos(w)*y[n-1] - R²*y[n-2] + x[n]
/// Cost: 2 multiplies + 2 adds per sample.
#[derive(Clone, Debug)]
struct Mode {
    /// Filter state
    y1: f32,
    y2: f32,
    /// Coefficients
    coeff: f32, // 2 * R * cos(w)
    r_sq: f32,  // R²
    /// Amplitude (includes position weighting)
    amp: f32,
}

impl Mode {
    fn new() -> Self {
        Self {
            y1: 0.0,
            y2: 0.0,
            coeff: 0.0,
            r_sq: 0.0,
            amp: 0.0,
        }
    }

    /// Set mode frequency and decay.
    /// `freq`: mode frequency as fraction of sample rate (0..0.5)
    /// `decay`: R value (0..1), higher = longer ring
    fn set(&mut self, freq: f32, decay: f32, amplitude: f32) {
        let w = 2.0 * core::f32::consts::PI * freq;
        self.coeff = 2.0 * decay * libm::cosf(w);
        self.r_sq = decay * decay;
        self.amp = amplitude;
    }

    /// Process one sample. Returns the mode's contribution.
    #[inline]
    fn tick(&mut self, input: f32) -> f32 {
        let y0 = input + self.coeff * self.y1 - self.r_sq * self.y2;
        self.y2 = self.y1;
        self.y1 = y0;
        y0 * self.amp
    }
}

/// Cheap recursive cosine oscillator for position weighting.
/// Produces cos(n * w) for successive n without per-mode trig.
struct CosineOsc {
    y0: f32,
    y1: f32,
    coeff: f32,
}

impl CosineOsc {
    /// Initialize for a given position (0..1 along the string/surface).
    fn init(&mut self, position: f32) {
        let w = core::f32::consts::PI * position;
        self.coeff = 2.0 * libm::cosf(w);
        self.y0 = 1.0; // cos(0)
        self.y1 = libm::cosf(w); // cos(w)
    }

    /// Get the next cosine value: cos(n*w) for successive n.
    #[inline]
    fn next(&mut self) -> f32 {
        let val = self.y0;
        let y_new = self.coeff * self.y0 - self.y1;
        self.y1 = self.y0;
        self.y0 = y_new;
        // Return absolute value — we want amplitude, not phase
        libm::fabsf(val)
    }
}

/// Modal synthesis parameters.
#[derive(Clone, Copy, Debug)]
pub struct ModalParams {
    /// Exciter amount / velocity sensitivity (0..1)
    pub excite: f32,
    /// Global decay time (0..1, maps to R = 0.99..0.99999)
    pub decay: f32,
    /// High-frequency damping (0..1, 0=bright, 1=very damped)
    pub damping: f32,
    /// Base pitch as MIDI note (set by note_on)
    pub note: f32,
    /// Brightness: amplitude rolloff of higher modes (0..1)
    pub brightness: f32,
    /// Excitation position along the string/surface (0..1)
    pub position: f32,
    /// Structure: inharmonicity (0=harmonic, 0.5=neutral, 1=metallic)
    pub inharm: f32,
    /// Number of active modes (4..24)
    pub num_modes: u8,
}

impl Default for ModalParams {
    fn default() -> Self {
        Self {
            excite: 0.8,
            decay: 0.6,
            damping: 0.3,
            note: 60.0,
            brightness: 0.5,
            position: 0.25, // 1/4 position — natural pluck point
            inharm: 0.0,    // harmonic
            num_modes: 16,
        }
    }
}

/// Modal resonator engine — bank of resonant bandpass filters.
/// Inspired by Mutable Instruments Rings.
pub struct ModalEngine {
    modes: [Mode; MAX_MODES],
    /// Exciter: remaining samples of noise burst
    exciter_remaining: usize,
    /// Exciter amplitude
    exciter_amp: f32,
    /// Simple noise state (xorshift)
    noise_state: u32,
    /// Whether any modes are still ringing
    active: bool,
    /// Sample counter for decay detection
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
            modes: core::array::from_fn(|_| Mode::new()),
            exciter_remaining: 0,
            exciter_amp: 0.0,
            noise_state: 0x12345678,
            active: false,
            silence_counter: 0,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, params: &ModalParams, sample_rate: u32) {
        let freq = note_to_freq(note);
        self.configure_modes(freq, params, sample_rate);

        // Start noise burst exciter (2-5ms depending on excite parameter)
        let burst_ms = 2.0 + params.excite * 3.0;
        self.exciter_remaining = (burst_ms * sample_rate as f32 / 1000.0) as usize;
        self.exciter_amp = velocity as f32 / 127.0 * params.excite * 0.5;
        self.active = true;
        self.silence_counter = 0;
    }

    pub fn note_off(&mut self) {
        // Modal sounds decay naturally — note_off just lets them ring out.
        // Could optionally apply extra damping here.
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Configure all mode frequencies, decay rates, and amplitudes.
    fn configure_modes(&mut self, base_freq: f32, params: &ModalParams, sample_rate: u32) {
        let num = (params.num_modes as usize).min(MAX_MODES);
        let sr = sample_rate as f32;

        // Base decay: R must be very close to 1 for audible ringing.
        // decay 0..1 maps to R = 0.9995..0.99999 (short ping → long sustain)
        let base_r = 0.9995 + params.decay * 0.00049;

        // Brightness: amplitude rolloff exponent (higher = darker)
        let rolloff = 1.0 + (1.0 - params.brightness) * 2.0;

        // Inharmonicity: stretch factor for mode spacing
        // 0 = perfect harmonics, 0.5 = neutral, 1 = stretched (metallic/bell)
        let stiffness = (params.inharm - 0.5) * 0.02;

        // Position-based amplitude weighting via cosine oscillator
        let mut cos_osc = CosineOsc {
            y0: 0.0,
            y1: 0.0,
            coeff: 0.0,
        };
        cos_osc.init(params.position);

        let mut stretch = 1.0_f32;

        for i in 0..MAX_MODES {
            if i >= num {
                self.modes[i].amp = 0.0;
                continue;
            }

            let harmonic = (i + 1) as f32;

            // Mode frequency with inharmonicity stretch
            stretch += stiffness * harmonic;
            let mode_freq = base_freq * harmonic * stretch;

            // Skip modes above Nyquist
            let freq_ratio = mode_freq / sr;
            if freq_ratio >= 0.49 {
                self.modes[i].amp = 0.0;
                continue;
            }

            // Per-mode decay: higher modes decay faster.
            // Damping reduces R slightly per harmonic — must stay very close to 1.
            let damping_per_mode = params.damping * 0.00002 * harmonic;
            let mode_r = (base_r - damping_per_mode).max(0.999).min(0.99999);

            // Amplitude: 1/n^rolloff, weighted by position
            let base_amp = 1.0 / libm::powf(harmonic, rolloff);
            let pos_weight = cos_osc.next();
            let amp = base_amp * pos_weight;

            self.modes[i].set(freq_ratio, mode_r, amp);
        }
    }

    /// Render a block of audio.
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

        let num = (params.num_modes as usize).min(MAX_MODES);
        let mut max_level = 0.0_f32;

        for s in output.iter_mut() {
            // Exciter: filtered noise burst
            let excite = if self.exciter_remaining > 0 {
                self.exciter_remaining -= 1;
                self.noise() * self.exciter_amp
            } else {
                0.0
            };

            // Sum all mode outputs
            let mut sum = 0.0;
            for mode in &mut self.modes[..num] {
                if mode.amp > 0.0001 {
                    sum += mode.tick(excite);
                }
            }

            // Scale and soft-limit to prevent harsh digital clipping
            let scaled = sum / (num as f32).max(1.0);
            *s = libm::tanhf(scaled * 2.0) * 0.5;
            max_level = max_level.max(libm::fabsf(*s));
        }

        // Detect silence (all modes decayed)
        if max_level < 0.0001 && self.exciter_remaining == 0 {
            self.silence_counter += 1;
            if self.silence_counter > 10 {
                self.active = false;
            }
        } else {
            self.silence_counter = 0;
        }
    }

    /// Fast xorshift noise generator.
    #[inline]
    fn noise(&mut self) -> f32 {
        self.noise_state ^= self.noise_state << 13;
        self.noise_state ^= self.noise_state >> 17;
        self.noise_state ^= self.noise_state << 5;
        // Convert to float in -1..1
        (self.noise_state as i32) as f32 / i32::MAX as f32
    }
}

fn note_to_freq(note: u8) -> f32 {
    440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0)
}
