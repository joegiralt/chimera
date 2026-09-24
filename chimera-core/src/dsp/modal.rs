use chimera_hal::BLOCK_SIZE;

use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::hw::Cost;

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
        Self {
            state_1: 0.0,
            state_2: 0.0,
            g: 0.0,
            r: 1.0,
            h: 1.0,
        }
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
        Self {
            y0: 0.0,
            y1: 0.0,
            iir_coefficient: 0.0,
            initial_amplitude: 0.0,
        }
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
    String = 0,      // Ambika KS+ (body, stiffness, position, ensemble)
    Modal = 1,       // SVF bandpass bank (Rings-style)
    Bowed = 2,       // Sustained bow friction
    Sympathetic = 3, // Multiple resonating strings (Rings-style)
}

impl ResonatorMode {
    pub fn from_u8(v: u8) -> Self {
        match v % 4 {
            0 => ResonatorMode::String,
            1 => ResonatorMode::Modal,
            2 => ResonatorMode::Bowed,
            _ => ResonatorMode::Sympathetic,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ModalParams {
    pub mode: ResonatorMode,
    pub excite: f32,
    pub decay: f32,
    pub brightness: f32,
    pub inharm: f32,   // Modal: stiffness. String: not used.
    pub position: f32, // Modal: excitation position. String: pluck position.
    pub note: f32,
    pub num_modes: u8,
    // String (KS+) params
    pub ks_excitation: u8, // 0=noise, 1=click, 2=bright, 3=dark
    pub ks_color: f32,     // excitation brightness
    pub ks_body: f32,      // body resonance (half-delay comb)
    pub ks_stiffness: f32, // allpass dispersion (bell character)
    pub ks_feedback: f32,  // sustain boost
    pub ks_ens_rate: f32,  // ensemble LFO rate
    pub ks_ens_depth: f32, // ensemble detuning depth
    pub ks_ens_mix: f32,   // ensemble dry/wet
    // Bowed params
    pub bow_velocity: f32,
    pub bow_force: f32,
}

impl Default for ModalParams {
    fn default() -> Self {
        Self {
            mode: ResonatorMode::String,
            excite: 0.8,
            decay: 0.3,
            brightness: 0.7,
            inharm: 0.25,
            position: 0.0, // bridge position
            note: 60.0,
            num_modes: 32,
            ks_excitation: 0, // noise
            ks_color: 0.8,
            ks_body: 0.3,
            ks_stiffness: 0.0,
            ks_feedback: 0.2,
            ks_ens_rate: 0.3,
            ks_ens_depth: 0.0,
            ks_ens_mix: 0.0,
            bow_velocity: 0.5,
            bow_force: 0.5,
        }
    }
}

impl ModalParams {
    pub const MODE: ParamId = ParamId(0);
    pub const EXCITE: ParamId = ParamId(1);
    pub const DECAY: ParamId = ParamId(2);
    pub const BRIGHTNESS: ParamId = ParamId(3);
    pub const POSITION: ParamId = ParamId(4);
    pub const INHARM: ParamId = ParamId(5);
    pub const KS_BODY: ParamId = ParamId(6);
    pub const KS_STIFFNESS: ParamId = ParamId(7);
    pub const KS_FEEDBACK: ParamId = ParamId(8);
    pub const KS_ENS_DEPTH: ParamId = ParamId(9);
    pub const KS_ENS_RATE: ParamId = ParamId(10);
    pub const KS_ENS_MIX: ParamId = ParamId(11);
}

/// Modal params are read at note-on (or by the engine from the unmodulated
/// snapshot), never from `Voice`'s modulated copy: none are modulatable.
/// Only UI-bound params have specs (plan D16). MODE max 3 is plan D3.
pub static MODAL_SPECS: [ParamSpec; 12] = [
    ParamSpec::choice(0, "MODE", ValFmt::Int(3), 3.0, 0.0),
    ParamSpec::continuous(1, "EXCITE", ValFmt::Uni, 0.0, 1.0, 0.8, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "DECAY", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "BRIGHT", ValFmt::Uni, 0.0, 1.0, 0.7, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "POS", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "INHARM", ValFmt::Uni, 0.0, 1.0, 0.25, 1.0 / 128.0, false),
    ParamSpec::continuous(6, "BODY", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(7, "STIFF", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(8, "FDBK", ValFmt::Uni, 0.0, 1.0, 0.2, 1.0 / 128.0, false),
    ParamSpec::continuous(9, "E.DPT", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(10, "E.RAT", ValFmt::Uni, 0.0, 1.0, 0.3, 1.0 / 128.0, false),
    ParamSpec::continuous(11, "E.MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for ModalParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &MODAL_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::MODE => self.mode as u8 as f32,
            Self::EXCITE => self.excite,
            Self::DECAY => self.decay,
            Self::BRIGHTNESS => self.brightness,
            Self::POSITION => self.position,
            Self::INHARM => self.inharm,
            Self::KS_BODY => self.ks_body,
            Self::KS_STIFFNESS => self.ks_stiffness,
            Self::KS_FEEDBACK => self.ks_feedback,
            Self::KS_ENS_DEPTH => self.ks_ens_depth,
            Self::KS_ENS_RATE => self.ks_ens_rate,
            Self::KS_ENS_MIX => self.ks_ens_mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::MODE => self.mode = ResonatorMode::from_u8(v as u8),
            Self::EXCITE => self.excite = v,
            Self::DECAY => self.decay = v,
            Self::BRIGHTNESS => self.brightness = v,
            Self::POSITION => self.position = v,
            Self::INHARM => self.inharm = v,
            Self::KS_BODY => self.ks_body = v,
            Self::KS_STIFFNESS => self.ks_stiffness = v,
            Self::KS_FEEDBACK => self.ks_feedback = v,
            Self::KS_ENS_DEPTH => self.ks_ens_depth = v,
            Self::KS_ENS_RATE => self.ks_ens_rate = v,
            Self::KS_ENS_MIX => self.ks_ens_mix = v,
            _ => {}
        }
    }
}

// ── Modal Engine ────────────────────────────────────────────────────

// ── Karplus-Strong delay line (ported from Ambika custom firmware) ───

/// String delay-line length (ADR 0014): the period of E1 (MIDI 28, 41.2 Hz)
/// at 48 kHz is 1,164 samples, so E1 and above play at their exact period;
/// lower notes clamp to 1,199 samples (~40 Hz). Sized so six voices fit D2.
pub const MAX_STRING_DELAY: usize = 1200;

/// Parameters for `KsString::tick_full`, built once per render block (not
/// per sample) at each call site.
/// damping: 0..1 (lowpass coefficient)
/// decay: 0..1 (AC attenuation rate)
/// body: 0..1 (half-delay comb resonance)
/// stiffness: 0..1 (allpass dispersion for bell character)
/// feedback: 0..1 (sustain boost)
/// ens_rate/ens_depth/ens_spread/ens_mix: ensemble chorus parameters
#[derive(Clone, Copy)]
pub struct KsRenderParams {
    pub damping: f32,
    pub decay: f32,
    pub body: f32,
    pub stiffness: f32,
    pub feedback: f32,
    pub ens_rate: f32,
    pub ens_depth: f32,
    pub ens_spread: f32,
    pub ens_mix: f32,
}

struct KsString {
    buffer: [f32; MAX_STRING_DELAY],
    write_pos: usize,
    delay_len: usize,
    ens_lfo_phase: u32,
    noise_state: u32,
}

impl KsString {
    fn new() -> Self {
        Self {
            buffer: [0.0; MAX_STRING_DELAY],
            write_pos: 0,
            delay_len: 100,
            ens_lfo_phase: 0,
            noise_state: 0x87654321,
        }
    }

    fn set_freq(&mut self, freq: f32, sample_rate: u32) {
        let period = sample_rate as f32 / freq;
        self.delay_len = (period as usize).clamp(2, MAX_STRING_DELAY - 1);
    }

    /// Excite the string (ported from Ambika Trigger).
    /// excitation: 0=noise, 1=click, 2=bright, 3=dark
    fn trigger(&mut self, amplitude: f32, excitation: u8, color: f32, position: f32) {
        // Fill delay line based on excitation type
        let mut prev = 0.0_f32;
        for i in 0..self.delay_len {
            let sample = match excitation % 4 {
                1 => {
                    // Click: short impulse
                    if i < 4 { amplitude } else { 0.0 }
                }
                2 => {
                    // Bright noise
                    let n1 = xorshift_noise(&mut self.noise_state);
                    let n2 = xorshift_noise(&mut self.noise_state);
                    (n1 * 0.5 + n2 * 0.25) * amplitude
                }
                3 => {
                    // Dark noise: average with previous
                    let n = xorshift_noise(&mut self.noise_state) * amplitude;
                    prev = (n + prev) * 0.5;
                    prev
                }
                _ => {
                    // White noise
                    xorshift_noise(&mut self.noise_state) * amplitude
                }
            };
            self.buffer[i] = sample;
        }

        // Pluck position: comb notch at position harmonics
        if position > 0.03 {
            let notch_period = ((self.delay_len as f32 * position) as usize).max(2);
            if notch_period < self.delay_len {
                for i in 0..self.delay_len - notch_period {
                    self.buffer[i] = (self.buffer[i] + self.buffer[i + notch_period]) * 0.5;
                }
            }
        }

        // Excitation color: low-pass filter passes (lower color = darker)
        let filter_passes = ((1.0 - color) * 7.0) as usize;
        for _ in 0..filter_passes {
            for i in 1..self.delay_len {
                self.buffer[i] = (self.buffer[i] + self.buffer[i - 1]) * 0.5;
            }
        }

        self.write_pos = 0;
        self.ens_lfo_phase = 0;
    }

    /// Full render with all Ambika KS features. See `KsRenderParams` for
    /// the field meanings.
    #[inline]
    fn tick_full(&mut self, p: &KsRenderParams) -> f32 {
        // Read position: one ahead of write
        let read_pos = (self.write_pos + 1) % self.delay_len;
        let current = self.buffer[read_pos];
        let next = self.buffer[(read_pos + 1) % self.delay_len];

        // KS low-pass averaging: blend between current and next sample.
        // Higher coeff = more averaging = darker sound.
        // damping=0 (bright): coeff=0.05 (barely any filtering)
        // damping=1 (dark): coeff=0.5 (heavy filtering, fast decay)
        let coeff = 0.05 + p.damping * 0.45;
        let mut filtered = current * (1.0 - coeff) + next * coeff;

        // The 2-point average inherently decays the signal.
        // Apply a per-sample gain < 1.0 to control decay time.
        // decay=0 → gain=0.9990 (very long ring, ~7 seconds)
        // decay=1 → gain=0.9900 (short pluck, ~100ms)
        let gain = 0.999 - p.decay * 0.009;
        filtered *= gain;

        // Stiffness: mix with a sample from +7 offset (allpass-like dispersion)
        if p.stiffness > 0.01 {
            let stiff_pos = (read_pos + 7) % self.delay_len;
            let stiff_sample = self.buffer[stiff_pos];
            filtered = filtered * (1.0 - p.stiffness) + stiff_sample * p.stiffness;
        }

        // Body resonance: comb filter at half-delay
        if p.body > 0.03 {
            let body_pos = (read_pos + self.delay_len / 2) % self.delay_len;
            let body_sample = self.buffer[body_pos];
            filtered = filtered * (1.0 - p.body * 0.5) + body_sample * p.body * 0.5;
        }

        // Feedback boost for sustain (adds energy back, fights decay)
        // Only at high values does it approach infinite sustain.
        if p.feedback > 0.01 {
            filtered += filtered * p.feedback * 0.3;
            filtered = filtered.clamp(-1.5, 1.5);
        }

        // Write back
        self.buffer[read_pos] = filtered;
        self.write_pos = read_pos;

        // Ensemble: three read heads with LFO detuning
        let mut output = filtered;
        if p.ens_mix > 0.01 && p.ens_depth > 0.01 {
            let lfo_inc = ((p.ens_rate + 0.01) * 1000.0) as u32;
            self.ens_lfo_phase = self.ens_lfo_phase.wrapping_add(lfo_inc);

            // Triangle LFO: 0..1..0..-1..0
            let lfo_raw = (self.ens_lfo_phase >> 16) as i16;
            let lfo_val = if self.ens_lfo_phase & 0x80000000 != 0 {
                -(lfo_raw as f32 / 32768.0)
            } else {
                lfo_raw as f32 / 32768.0
            };

            let offset2 = (lfo_val * p.ens_depth * self.delay_len as f32 * 0.05) as i32;
            let offset3 = -offset2 + (p.ens_spread * self.delay_len as f32 * 0.02) as i32;

            let p2 = ((read_pos as i32 + offset2).rem_euclid(self.delay_len as i32)) as usize;
            let p3 = ((read_pos as i32 + offset3).rem_euclid(self.delay_len as i32)) as usize;

            let head2 = self.buffer[p2];
            let head3 = self.buffer[p3];

            output = filtered * (1.0 - p.ens_mix) + (head2 + head3) * 0.5 * p.ens_mix;
        }

        output
    }
}

// ── Modal Engine (with String and Bowed modes) ──────────────────────

const NUM_SYMPATHETIC: usize = 7;

pub struct ModalEngine {
    filters: [Svf; MAX_MODES],
    cos_osc: CosineOsc,
    resolution: usize,
    // String model (main)
    string: KsString,
    // Sympathetic strings (7 additional resonators)
    sym_strings: [KsString; NUM_SYMPATHETIC],
    // Bowed model state
    bow_state: f32,
    // Shared
    frequency: f32,
    active_mode: ResonatorMode,
    released: bool, // true after note_off
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
    /// Design doc § CPU Budget: physical modeling (modal, 8 modes) ~800.
    pub const COST: Cost = Cost(800); // estimate

    pub fn new() -> Self {
        Self {
            filters: core::array::from_fn(|_| Svf::new()),
            cos_osc: CosineOsc::new(),
            resolution: 0,
            string: KsString::new(),
            sym_strings: core::array::from_fn(|_| KsString::new()),
            bow_state: 0.0,
            frequency: 220.0 / 48000.0,
            active_mode: ResonatorMode::Modal,
            released: false,
            exciter_remaining: 0,
            exciter_amp: 0.0,
            noise_state: 0x12345678,
            exciter_lp: 0.0,
            active: false,
            silence_counter: 0,
        }
    }

    pub fn note_on(&mut self, note: u8, velocity: u8, params: &ModalParams, sample_rate: u32) {
        self.active_mode = params.mode;
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
                self.string.set_freq(freq, sample_rate);
                self.string.trigger(
                    vel * params.excite,
                    params.ks_excitation,
                    params.ks_color,
                    params.position,
                );
            }
            ResonatorMode::Bowed => {
                self.string.set_freq(freq, sample_rate);
                for s in self.string.buffer.iter_mut() {
                    *s = 0.0;
                }
                self.bow_state = 0.0;
                self.exciter_amp = vel * params.bow_force;
            }
            ResonatorMode::Sympathetic => {
                // Main string gets excitation
                self.string.set_freq(freq, sample_rate);
                self.string.trigger(
                    vel * params.excite,
                    params.ks_excitation,
                    params.ks_color,
                    params.position,
                );
                // Sympathetic strings tuned to harmonics/intervals
                // inharm controls spread: 0=unison, 1=wide harmonic series
                let intervals = [0.0, 12.0, 7.02, 12.0, 19.02, 24.0, 7.02];
                for (i, sym) in self.sym_strings.iter_mut().enumerate() {
                    let detune = intervals[i] * params.inharm;
                    let sym_freq = freq * semitones_to_ratio(detune);
                    sym.set_freq(sym_freq, sample_rate);
                    // Sympathetic strings start silent — energy comes from main
                    for s in sym.buffer[..sym.delay_len].iter_mut() {
                        *s = 0.0;
                    }
                    sym.write_pos = 0;
                }
            }
        }

        self.active = true;
        self.released = false;
        self.silence_counter = 0;
    }

    pub fn note_off(&mut self) {
        self.released = true;
        match self.active_mode {
            ResonatorMode::String => {
                // Dampen the buffer heavily
                for _ in 0..3 {
                    for i in 0..self.string.delay_len {
                        self.string.buffer[i] *= 0.2;
                    }
                }
            }
            ResonatorMode::Bowed => {
                // Stop the bow — zero exciter, heavily dampen string
                self.exciter_amp = 0.0;
                for _ in 0..5 {
                    for i in 0..self.string.delay_len {
                        self.string.buffer[i] *= 0.2;
                    }
                }
            }
            ResonatorMode::Modal => {}
            ResonatorMode::Sympathetic => {
                for i in 0..self.string.delay_len {
                    self.string.buffer[i] *= 0.2;
                }
                for sym in &mut self.sym_strings {
                    for i in 0..sym.delay_len {
                        sym.buffer[i] *= 0.2;
                    }
                }
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Configure filters — called every render block (not just note_on).
    /// Matches Rings' ComputeFilters().
    fn compute_filters(&mut self, params: &ModalParams, frequency: f32) {
        let num = (params.num_modes as usize).min(MAX_MODES) & !1;
        self.resolution = num;

        // Q from decay (Rings-style range).
        // At partial_freq=0.003 (130Hz): mode_q = 1 + 0.003 * q
        //   decay=0:   q=500,    mode_q=2.5  (short ping)
        //   decay=0.5: q=50000,  mode_q=151  (nice ring)
        //   decay=1:   q=500000, mode_q=1501 (long sustain)
        let mut q = 500.0 * libm::powf(10.0, params.decay * 3.0); // 500..500,000

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

        for filter in self.filters.iter_mut().take(num) {
            let partial_freq = (harmonic * stretch_factor).min(0.49);

            // Per-mode Q (Rings: 1.0 + partial_freq * q)
            let mode_q = 1.0 + partial_freq * q;
            filter.set(partial_freq, mode_q);

            // Accumulate stiffness with decay for negative values
            stretch_factor += stiffness;
            if stretch_factor < 0.1 {
                stretch_factor = 0.1;
            } // never go negative
            if stiffness < 0.0 {
                stiffness *= 0.93;
            } // decay negative stiffness

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
            ResonatorMode::String => self.render_string(output, params, &mut max_level),
            ResonatorMode::Modal => self.render_modal(output, &mut max_level),
            ResonatorMode::Bowed => self.render_bowed(output, params, &mut max_level),
            ResonatorMode::Sympathetic => self.render_sympathetic(output, params, &mut max_level),
        }

        if max_level < 0.001 && self.exciter_remaining == 0 {
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
                let raw = xorshift_noise(&mut self.noise_state) * self.exciter_amp * env;
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

            // Sum to mono, scale up, soft-limit
            *s = libm::tanhf(odd + even) * 2.0;
            *max_level = max_level.max(libm::fabsf(*s));
        }
    }

    fn render_string(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ModalParams,
        max_level: &mut f32,
    ) {
        let (fb, body, stiff, decay) = if self.released {
            (0.0, 0.0, 0.0, 0.8_f32.max(params.decay)) // fast decay on release
        } else {
            (
                params.ks_feedback,
                params.ks_body,
                params.ks_stiffness,
                params.decay,
            )
        };
        let render_params = KsRenderParams {
            damping: params.brightness,
            decay,
            body,
            stiffness: stiff,
            feedback: fb,
            ens_rate: params.ks_ens_rate,
            ens_depth: params.ks_ens_depth,
            ens_spread: 0.3, // fixed for now
            ens_mix: params.ks_ens_mix,
        };
        for s in output.iter_mut() {
            *s = self.string.tick_full(&render_params);
            *max_level = max_level.max(libm::fabsf(*s));
        }
    }

    fn render_bowed(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ModalParams,
        max_level: &mut f32,
    ) {
        let bow_vel = if self.exciter_amp > 0.001 {
            params.bow_velocity * 0.3
        } else {
            0.0
        };
        let bow_force = self.exciter_amp * 4.0;
        // When bow is released, apply decay
        let release_decay = if self.exciter_amp < 0.001 { 0.995 } else { 1.0 };

        for s in output.iter_mut() {
            // Read from delay line
            let read_pos = (self.string.write_pos + MAX_STRING_DELAY - self.string.delay_len) % MAX_STRING_DELAY;
            let string_vel = self.string.buffer[read_pos];

            // Bow friction: stick-slip model.
            // When |delta_v| is small, bow sticks (high friction → energy in).
            // When |delta_v| is large, bow slips (low friction → string rings free).
            let delta_v = bow_vel - string_vel;
            let friction = bow_force * libm::tanhf(delta_v * 8.0);

            let feedback = string_vel * 0.9995 * release_decay + friction * 0.4;

            // Soft-limit to prevent blowup
            let clamped = libm::tanhf(feedback);

            self.string.buffer[self.string.write_pos] = clamped;
            self.string.write_pos = (self.string.write_pos + 1) % MAX_STRING_DELAY;

            *s = string_vel;
            *max_level = max_level.max(libm::fabsf(*s));
        }
    }

    fn render_sympathetic(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &ModalParams,
        max_level: &mut f32,
    ) {
        let released = self.released;
        let (fb, body, stiff) = if released {
            (0.0, 0.0, 0.0)
        } else {
            (params.ks_feedback, params.ks_body, params.ks_stiffness)
        };
        let decay = if released {
            0.8_f32.max(params.decay)
        } else {
            params.decay
        };

        // Coupling gain: how much main string feeds into sympathetic
        let coupling = 0.025; // Rings uses 0.2 / num_strings

        let main_params = KsRenderParams {
            damping: params.brightness,
            decay,
            body,
            stiffness: stiff,
            feedback: fb,
            ens_rate: params.ks_ens_rate,
            ens_depth: params.ks_ens_depth,
            ens_spread: 0.3,
            ens_mix: params.ks_ens_mix,
        };
        let sym_params = KsRenderParams {
            damping: params.brightness * 0.7, // darker
            decay: decay * 0.5,               // slower decay
            body: 0.0,
            stiffness: 0.0,
            feedback: 0.0, // no body/stiff/feedback
            ens_rate: 0.0,
            ens_depth: 0.0,
            ens_spread: 0.0,
            ens_mix: 0.0, // no ensemble
        };

        for s in output.iter_mut() {
            // 1. Main string tick
            let main_out = self.string.tick_full(&main_params);

            // 2. Couple main string output into sympathetic strings
            let sym_input = main_out * coupling;

            // 3. Tick all sympathetic strings, sum their output
            let mut sym_sum = 0.0_f32;
            for sym in &mut self.sym_strings {
                // Inject coupled energy from main string into delay line
                let wp = sym.write_pos;
                sym.buffer[wp] += sym_input;
                // Tick the sympathetic string (with gentler damping)
                let sym_out = sym.tick_full(&sym_params);
                sym_sum += sym_out;
            }

            // 4. Mix: main + sympathetic
            let mixed = main_out + sym_sum * 0.15;
            *s = libm::tanhf(mixed);
            *max_level = max_level.max(libm::fabsf(*s));
        }
    }

}

use super::note_to_freq;

#[inline]
fn xorshift_noise(state: &mut u32) -> f32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    (*state as i32) as f32 / i32::MAX as f32
}

fn semitones_to_ratio(semitones: f32) -> f32 {
    libm::powf(2.0, semitones / 12.0)
}
