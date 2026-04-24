//! FM operator with oscillator, envelope, and modulation.
//!
//! Ported from the p81z TX81Z emulator (TX81Z_oscillator.cpp).
//! Each operator has its own phase accumulator, waveform selector, feedback
//! path, and envelope generator. The `run` / `run_adding` interface matches
//! the p81z "internalRun" loop.

use crate::dsp::envelope_fm::FmEnvelope;
use crate::dsp::fm_tables;
use crate::dsp::fm_waveform;

/// Per-operator settings (matches TX81Z voice parameters).
#[derive(Clone, Debug)]
pub struct FmOpSettings {
    pub waveform: u8,       // 0-7
    pub coarse: u8,         // 0-63
    pub fine: u8,           // 0-15
    pub level: u8,          // 0-99
    pub feedback: u8,       // 0-7
    pub detune: i8,         // -7..+7
    pub velocity_sens: u8,  // 0-7
    pub ar: u8,
    pub d1r: u8,
    pub d1l: u8,
    pub d2r: u8,
    pub rr: u8,
    pub rate_scaling: u8,
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
    pub fn note_on(
        &mut self,
        note: u8,
        velocity: f32,
        settings: &FmOpSettings,
        sample_rate: f32,
    ) {
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
