//! FM operator with oscillator, envelope, and modulation.
//!
//! Ported from the p81z TX81Z emulator (TX81Z_oscillator.cpp).
//! Each operator has its own phase accumulator, waveform selector, feedback
//! path, and envelope generator. The `run` / `run_adding` interface matches
//! the p81z "internalRun" loop.

use chimera_hal::BLOCK_SIZE;

use crate::dsp::envelope_fm::FmEnvelope;
use crate::dsp::fm_tables;
use crate::dsp::fm_waveform;
use crate::hw::Cost;

/// Per-operator settings (matches TX81Z voice parameters).
#[derive(Clone, Copy, Debug)]
pub struct FmOpSettings {
    pub waveform: u8,      // 0-7
    pub coarse: u8,        // 0-63
    pub fine: u8,          // 0-15
    pub level: u8,         // 0-99
    pub feedback: u8,      // 0-7
    pub detune: i8,        // -7..+7
    pub velocity_sens: u8, // 0-7
    pub ar: u8,
    pub d1r: u8,
    pub d1l: u8,
    pub d2r: u8,
    pub rr: u8,
    pub rate_scaling: u8,
}

impl FmOpSettings {
    /// Convert from [`crate::params::FmOpParams`] to this settings struct.
    pub fn from_params(p: &crate::params::FmOpParams) -> Self {
        Self {
            waveform: p.waveform,
            coarse: p.coarse,
            fine: p.fine,
            level: p.level as u8,
            feedback: p.feedback as u8,
            detune: p.detune,
            velocity_sens: p.velocity_sens,
            ar: p.attack_rate,
            d1r: p.decay1_rate,
            d1l: p.decay1_level,
            d2r: p.decay2_rate,
            rr: p.release_rate,
            rate_scaling: p.rate_scaling,
        }
    }
}

impl Default for FmOpSettings {
    fn default() -> Self {
        Self {
            waveform: 0,
            coarse: 4, // ratio 1.0
            fine: 0,
            level: 99,
            feedback: 0,
            detune: 0,
            velocity_sens: 0,
            ar: 31,
            d1r: 0,
            d1l: 15,
            d2r: 0,
            rr: 15,
            rate_scaling: 0,
        }
    }
}

/// A single FM operator: oscillator + envelope + feedback.
#[derive(Clone, Debug)]
pub struct FmOperator {
    phase: f32,
    phase_increment: f32,
    prev_output: f32,
    envelope: FmEnvelope,
    gain: f32,
    feedback_amount: f32,
    waveform: u8,
    active: bool,
}

impl Default for FmOperator {
    fn default() -> Self {
        Self::new()
    }
}

impl FmOperator {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            phase_increment: 0.0,
            prev_output: 0.0,
            envelope: FmEnvelope::new(),
            gain: 0.0,
            feedback_amount: 0.0,
            waveform: 0,
            active: false,
        }
    }

    /// Trigger the operator with MIDI note and velocity.
    ///
    /// Computes frequency from note + coarse/fine ratio + detune, sets gain
    /// from operator level, configures feedback, and starts the envelope.
    pub fn note_on(&mut self, note: u8, velocity: f32, settings: &FmOpSettings, sample_rate: f32) {
        // Base frequency from MIDI note
        let base_freq = 440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0);

        // Apply coarse + fine ratio
        let ratio = fm_tables::compute_ratio(settings.coarse as usize, settings.fine as usize);
        let mut frequency = base_freq * ratio;

        // Apply detune (cents-style: each detune step ~1.04 cents)
        if settings.detune != 0 {
            frequency *= libm::powf(2.0, settings.detune as f32 / (12.0 * 64.0));
        }

        self.phase_increment = frequency / sample_rate;
        self.phase = 0.0;
        self.prev_output = 0.0;
        self.waveform = settings.waveform;

        // Gain from operator output level, scaled by velocity sensitivity
        let vel_factor =
            fm_tables::compute_velocity_factor(settings.velocity_sens as usize, velocity);
        self.gain = fm_tables::level_to_gain(settings.level) * vel_factor;

        // Feedback
        self.feedback_amount = fm_tables::FEEDBACK[settings.feedback.min(7) as usize];

        // Start envelope
        self.envelope.note_on(
            settings.ar,
            settings.d1r,
            settings.d1l,
            settings.d2r,
            settings.rr,
            settings.velocity_sens as usize,
            settings.rate_scaling as usize,
            sample_rate,
            note,
        );

        self.active = true;
    }

    /// Update live-tweakable params without resetting phase or envelope.
    /// Called each render block so encoder changes are heard immediately.
    pub fn update_live(&mut self, settings: &FmOpSettings) {
        self.gain = fm_tables::level_to_gain(settings.level);
        self.feedback_amount = fm_tables::FEEDBACK[settings.feedback.min(7) as usize];
        self.waveform = settings.waveform;
    }

    /// Release the operator (envelope enters release stage).
    pub fn note_off(&mut self) {
        self.envelope.note_off();
    }

    /// Returns `true` when the operator has finished sounding (envelope idle).
    #[inline]
    pub fn is_idle(&self) -> bool {
        !self.active || self.envelope.is_idle()
    }

    /// Render operator output, replacing contents of `audio_out`.
    ///
    /// `mod_in` contains phase-modulation input from other operators (or zeros).
    /// Mirrors the p81z `internalRun` loop.
    #[allow(clippy::needless_range_loop)]
    pub fn run(&mut self, mod_in: &[f32], audio_out: &mut [f32]) {
        let len = mod_in.len().min(audio_out.len());

        // Generate envelope amplitude into a stack buffer, processing in chunks
        // to avoid large stack allocations.
        const CHUNK: usize = 256;
        let mut env_buf = [0.0f32; CHUNK];
        let mut offset = 0;

        while offset < len {
            let n = (len - offset).min(CHUNK);
            self.envelope.run(&mut env_buf[..n]);

            for i in 0..n {
                let idx = offset + i;

                // Phase accumulate
                self.phase += self.phase_increment;
                // Wrap to 0.0-1.0 (matching p81z: phase -= (int)phase)
                self.phase -= self.phase as i32 as f32;

                // Phase modulation: carrier phase + modulator input + self-feedback
                let mut phase_mod = self.phase
                    + (fm_tables::OPERATOR_FACTOR * mod_in[idx])
                    + (self.prev_output * self.feedback_amount);

                // Wrap to 0.0-1.0
                phase_mod -= phase_mod as i32 as f32;
                if phase_mod < 0.0 {
                    phase_mod += 1.0;
                }

                // Waveform computation (replaces p81z wavetable lookup)
                let wave_value = fm_waveform::compute(self.waveform, phase_mod);

                // Apply envelope amplitude
                let osc_value = env_buf[i] * wave_value;
                self.prev_output = osc_value;

                // Write output with gain
                audio_out[idx] = self.gain * osc_value;
            }

            offset += n;
        }

        // Check if envelope has gone idle
        if self.envelope.is_idle() {
            self.active = false;
        }
    }

    /// Render operator output, adding to existing contents of `audio_out`.
    ///
    /// Same as `run` but uses `+=` instead of `=` for the output.
    #[allow(clippy::needless_range_loop)]
    pub fn run_adding(&mut self, mod_in: &[f32], audio_out: &mut [f32]) {
        let len = mod_in.len().min(audio_out.len());

        const CHUNK: usize = 256;
        let mut env_buf = [0.0f32; CHUNK];
        let mut offset = 0;

        while offset < len {
            let n = (len - offset).min(CHUNK);
            self.envelope.run(&mut env_buf[..n]);

            for i in 0..n {
                let idx = offset + i;

                self.phase += self.phase_increment;
                self.phase -= self.phase as i32 as f32;

                let mut phase_mod = self.phase
                    + (fm_tables::OPERATOR_FACTOR * mod_in[idx])
                    + (self.prev_output * self.feedback_amount);

                phase_mod -= phase_mod as i32 as f32;
                if phase_mod < 0.0 {
                    phase_mod += 1.0;
                }

                let wave_value = fm_waveform::compute(self.waveform, phase_mod);
                let osc_value = env_buf[i] * wave_value;
                self.prev_output = osc_value;

                audio_out[idx] += self.gain * osc_value;
            }

            offset += n;
        }

        if self.envelope.is_idle() {
            self.active = false;
        }
    }
}

// ---------------------------------------------------------------------------
// FmEngine — routes 4 operators through 8 TX81Z algorithms
// ---------------------------------------------------------------------------

/// Four-operator FM engine with 8 TX81Z algorithm topologies.
///
/// Uses named operator fields to avoid borrow-checker issues with
/// simultaneous mutable borrows of array elements.
pub struct FmEngine {
    op1: FmOperator,
    op2: FmOperator,
    op3: FmOperator,
    op4: FmOperator,
    temp: [f32; BLOCK_SIZE],
    temp2: [f32; BLOCK_SIZE],
    zeros: [f32; BLOCK_SIZE],
}

impl Default for FmEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl FmEngine {
    /// Design doc § CPU Budget: 4-op FM ~200 cycles/sample.
    pub const COST: Cost = Cost(200); // estimate

    pub fn new() -> Self {
        Self {
            op1: FmOperator::new(),
            op2: FmOperator::new(),
            op3: FmOperator::new(),
            op4: FmOperator::new(),
            temp: [0.0; BLOCK_SIZE],
            temp2: [0.0; BLOCK_SIZE],
            zeros: [0.0; BLOCK_SIZE],
        }
    }

    /// Trigger all 4 operators with a MIDI note.
    pub fn note_on(
        &mut self,
        note: u8,
        velocity: f32,
        _algorithm: u8,
        settings: &[FmOpSettings; 4],
        sample_rate: f32,
    ) {
        self.op1.note_on(note, velocity, &settings[0], sample_rate);
        self.op2.note_on(note, velocity, &settings[1], sample_rate);
        self.op3.note_on(note, velocity, &settings[2], sample_rate);
        self.op4.note_on(note, velocity, &settings[3], sample_rate);
    }

    /// Release all 4 operators.
    pub fn note_off(&mut self) {
        self.op1.note_off();
        self.op2.note_off();
        self.op3.note_off();
        self.op4.note_off();
    }

    /// Returns `true` when all operators have finished sounding.
    pub fn is_idle(&self) -> bool {
        self.op1.is_idle() && self.op2.is_idle() && self.op3.is_idle() && self.op4.is_idle()
    }

    /// Trigger all 4 operators using [`crate::params::FmParams`].
    pub fn note_on_params(
        &mut self,
        note: u8,
        velocity: f32,
        params: &crate::params::FmParams,
        sample_rate: f32,
    ) {
        let alg = params.algorithm;
        let settings: [FmOpSettings; 4] =
            core::array::from_fn(|i| FmOpSettings::from_params(&params.operators[i]));
        self.note_on(note, velocity, alg, &settings, sample_rate);
    }

    /// Render audio using [`crate::params::FmParams`].
    pub fn render_params(&mut self, output: &mut [f32], params: &crate::params::FmParams) {
        let alg = params.algorithm;
        let settings: [FmOpSettings; 4] =
            core::array::from_fn(|i| FmOpSettings::from_params(&params.operators[i]));
        // Update live-tweakable params so encoder changes are heard immediately
        self.op1.update_live(&settings[0]);
        self.op2.update_live(&settings[1]);
        self.op3.update_live(&settings[2]);
        self.op4.update_live(&settings[3]);
        self.render(output, alg, &settings);
    }

    /// Render audio into `output` using the given algorithm routing.
    ///
    /// Processes in BLOCK_SIZE chunks. The algorithm index (0-7) selects the
    /// operator topology, matching p81z `FMArrangement.cpp` except ALG 4
    /// (index 3), which follows the TX81Z (ADR 0018).
    ///
    /// Where p81z does `op.run(temp, temp)` (read and write same buffer),
    /// we use `temp2` as an intermediate to satisfy the borrow checker,
    /// then copy back.
    pub fn render(&mut self, output: &mut [f32], algorithm: u8, _settings: &[FmOpSettings; 4]) {
        let total = output.len();
        let mut pos = 0;

        while pos < total {
            let n = (total - pos).min(BLOCK_SIZE);
            let out = &mut output[pos..pos + n];

            // Route operators according to algorithm
            match algorithm {
                0 => {
                    // 4 -> 3 -> 2 -> [1]
                    self.op4.run(&self.zeros[..n], &mut self.temp[..n]);
                    // op3.run(temp, temp): read temp as mod, write temp as output
                    self.op3.run(&self.temp[..n], &mut self.temp2[..n]);
                    self.temp[..n].copy_from_slice(&self.temp2[..n]);
                    // op2.run(temp, temp)
                    self.op2.run(&self.temp[..n], &mut self.temp2[..n]);
                    self.temp[..n].copy_from_slice(&self.temp2[..n]);
                    self.op1.run(&self.temp[..n], out);
                }
                1 => {
                    // (3+4) -> 2 -> [1]
                    self.op3.run(&self.zeros[..n], &mut self.temp[..n]);
                    self.op4.run_adding(&self.zeros[..n], &mut self.temp[..n]);
                    // op2.run(temp, temp)
                    self.op2.run(&self.temp[..n], &mut self.temp2[..n]);
                    self.temp[..n].copy_from_slice(&self.temp2[..n]);
                    self.op1.run(&self.temp[..n], out);
                }
                2 => {
                    // 3 -> 2, (2+4) -> [1]
                    self.op3.run(&self.zeros[..n], &mut self.temp[..n]);
                    // op2.run(temp, temp)
                    self.op2.run(&self.temp[..n], &mut self.temp2[..n]);
                    self.temp[..n].copy_from_slice(&self.temp2[..n]);
                    self.op4.run_adding(&self.zeros[..n], &mut self.temp[..n]);
                    self.op1.run(&self.temp[..n], out);
                }
                3 => {
                    // 4 -> 3, (3 + 2) -> [1]. TX81Z ALG 4: op2 is unmodulated
                    // (p81z modulates op2 with op3 here; #18, ADR 0018).
                    self.op4.run(&self.zeros[..n], &mut self.temp[..n]);
                    // op3.run(temp, temp)
                    self.op3.run(&self.temp[..n], &mut self.temp2[..n]);
                    self.temp[..n].copy_from_slice(&self.temp2[..n]);
                    self.op2.run_adding(&self.zeros[..n], &mut self.temp[..n]);
                    self.op1.run(&self.temp[..n], out);
                }
                4 => {
                    // 2 -> [1], 4 -> [3]
                    self.op2.run(&self.zeros[..n], &mut self.temp[..n]);
                    self.op1.run(&self.temp[..n], out);
                    self.op4.run(&self.zeros[..n], &mut self.temp[..n]);
                    self.op3.run_adding(&self.temp[..n], out);
                }
                5 => {
                    // 4 -> [1], 4 -> [2], 4 -> [3]
                    self.op4.run(&self.zeros[..n], &mut self.temp[..n]);
                    self.op1.run(&self.temp[..n], out);
                    self.op2.run_adding(&self.temp[..n], out);
                    self.op3.run_adding(&self.temp[..n], out);
                }
                6 => {
                    // [1], [2], 4 -> [3]
                    self.op1.run(&self.zeros[..n], out);
                    self.op2.run_adding(&self.zeros[..n], out);
                    self.op4.run(&self.zeros[..n], &mut self.temp[..n]);
                    self.op3.run_adding(&self.temp[..n], out);
                }
                _ => {
                    // [1], [2], [3], [4]
                    self.op1.run(&self.zeros[..n], out);
                    self.op2.run_adding(&self.zeros[..n], out);
                    self.op3.run_adding(&self.zeros[..n], out);
                    self.op4.run_adding(&self.zeros[..n], out);
                }
            }

            pos += n;
        }
    }
}
