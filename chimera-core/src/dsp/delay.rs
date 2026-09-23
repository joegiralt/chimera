//! Tape-style delay with wow/flutter, saturation, and high-frequency rolloff.
//! Inspired by Roland Space Echo / analog tape delay character.

use chimera_hal::BLOCK_SIZE;
use crate::block::{Block, ParamId, ParamSpec, ValFmt};

/// Max delay: ~1 second at 48kHz
const MAX_DELAY_SAMPLES: usize = 48000;

/// Tape delay parameters.
#[derive(Clone, Copy, Debug)]
pub struct DelayParams {
    /// Delay time in ms (10..1000)
    pub time_ms: f32,
    /// Feedback amount (0..1)
    pub feedback: f32,
    /// Wow & flutter depth (0..1) — tape speed instability
    pub wow_flutter: f32,
    /// Tape saturation amount (0..1) — soft clipping in feedback path
    pub saturation: f32,
    /// Tone: high-frequency rolloff in feedback (0..1, 0=dark, 1=bright)
    pub tone: f32,
    /// Dry/wet mix (0..1)
    pub mix: f32,
}

impl Default for DelayParams {
    fn default() -> Self {
        Self {
            time_ms: 375.0, // ~1/8 note at 120bpm
            feedback: 0.4,
            wow_flutter: 0.15,
            saturation: 0.2,
            tone: 0.6,
            mix: 0.0, // off by default
        }
    }
}

impl DelayParams {
    pub const TIME_MS: ParamId = ParamId(0);
    pub const FEEDBACK: ParamId = ParamId(1);
    pub const WOW_FLUTTER: ParamId = ParamId(2);
    pub const SATURATION: ParamId = ParamId(3);
    pub const TONE: ParamId = ParamId(4);
    pub const MIX: ParamId = ParamId(5);
}

/// Delay runs outside `Voice` (desktop only): nothing is modulatable.
pub static DELAY_SPECS: [ParamSpec; 6] = [
    ParamSpec::continuous(0, "TIME", ValFmt::Uni, 10.0, 1000.0, 375.0, 8.0, false),
    ParamSpec::continuous(1, "FDBK", ValFmt::Uni, 0.0, 1.0, 0.4, 1.0 / 128.0, false),
    ParamSpec::continuous(2, "WOW", ValFmt::Uni, 0.0, 1.0, 0.15, 1.0 / 128.0, false),
    ParamSpec::continuous(3, "SAT", ValFmt::Uni, 0.0, 1.0, 0.2, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "TONE", ValFmt::Uni, 0.0, 1.0, 0.6, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "MIX", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
];

impl Block for DelayParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &DELAY_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::TIME_MS => self.time_ms,
            Self::FEEDBACK => self.feedback,
            Self::WOW_FLUTTER => self.wow_flutter,
            Self::SATURATION => self.saturation,
            Self::TONE => self.tone,
            Self::MIX => self.mix,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::TIME_MS => self.time_ms = v,
            Self::FEEDBACK => self.feedback = v,
            Self::WOW_FLUTTER => self.wow_flutter = v,
            Self::SATURATION => self.saturation = v,
            Self::TONE => self.tone = v,
            Self::MIX => self.mix = v,
            _ => {}
        }
    }
}

pub struct TapeDelay {
    buffer: [f32; MAX_DELAY_SAMPLES],
    write_pos: usize,
    /// LP filter state for tone control in feedback
    lp_state: f32,
    /// Wow LFO (slow, ~0.5Hz)
    wow_phase: f32,
    /// Flutter LFO (faster, ~6Hz)
    flutter_phase: f32,
}

impl Default for TapeDelay {
    fn default() -> Self {
        Self::new()
    }
}

impl TapeDelay {
    pub fn new() -> Self {
        Self {
            buffer: [0.0; MAX_DELAY_SAMPLES],
            write_pos: 0,
            lp_state: 0.0,
            wow_phase: 0.0,
            flutter_phase: 0.0,
        }
    }

    pub fn process(&mut self, buf: &mut [f32; BLOCK_SIZE], params: &DelayParams, sample_rate: u32) {
        if params.mix < 0.001 {
            return;
        }

        let base_delay = (params.time_ms * sample_rate as f32 / 1000.0)
            .clamp(1.0, (MAX_DELAY_SAMPLES - 2) as f32);

        let wow_rate = 0.5 / sample_rate as f32; // ~0.5 Hz
        let flutter_rate = 6.0 / sample_rate as f32; // ~6 Hz

        // Tone: LP coefficient (higher = brighter)
        let lp_coeff = 0.2 + params.tone * 0.75;

        for s in buf.iter_mut() {
            let dry = *s;

            // Wow & flutter: modulate delay time
            self.wow_phase += wow_rate;
            if self.wow_phase >= 1.0 {
                self.wow_phase -= 1.0;
            }
            self.flutter_phase += flutter_rate;
            if self.flutter_phase >= 1.0 {
                self.flutter_phase -= 1.0;
            }

            let wow = libm::sinf(self.wow_phase * 2.0 * core::f32::consts::PI);
            let flutter = libm::sinf(self.flutter_phase * 2.0 * core::f32::consts::PI);
            let mod_amount = params.wow_flutter * 20.0; // up to ±20 samples modulation
            let delay = base_delay + wow * mod_amount * 0.7 + flutter * mod_amount * 0.3;
            let delay = delay.clamp(1.0, (MAX_DELAY_SAMPLES - 2) as f32);

            // Interpolated read from delay line
            let d_int = delay as usize;
            let d_frac = delay - d_int as f32;
            let pos_a = (self.write_pos + MAX_DELAY_SAMPLES - d_int) % MAX_DELAY_SAMPLES;
            let pos_b = (self.write_pos + MAX_DELAY_SAMPLES - d_int - 1) % MAX_DELAY_SAMPLES;
            let delayed = self.buffer[pos_a] * (1.0 - d_frac) + self.buffer[pos_b] * d_frac;

            // Tone: one-pole LP in feedback path (tape loses highs each pass)
            self.lp_state += lp_coeff * (delayed - self.lp_state);
            let filtered = self.lp_state;

            // Tape saturation in feedback path
            let saturated = if params.saturation > 0.01 {
                let gain = 1.0 + params.saturation * 3.0;
                libm::tanhf(filtered * gain) / gain
            } else {
                filtered
            };

            // Write: input + feedback
            self.buffer[self.write_pos] = dry + saturated * params.feedback;
            self.write_pos = (self.write_pos + 1) % MAX_DELAY_SAMPLES;

            // Mix
            *s = dry * (1.0 - params.mix) + delayed * params.mix;
        }
    }
}
