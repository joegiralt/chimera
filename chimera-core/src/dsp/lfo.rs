/// LFO — Low Frequency Oscillator for modulation.
/// Outputs -1.0 to +1.0 (bipolar) at sub-audio rates.

use crate::block::{Block, ParamId, ParamSpec, ValFmt};
use crate::dsp::fast_sin;

/// LFO waveform shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoShape {
    Sine = 0,
    Triangle = 1,
    Saw = 2,
    Square = 3,
    Random = 4,  // sample & hold
}

impl LfoShape {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => LfoShape::Sine,
            1 => LfoShape::Triangle,
            2 => LfoShape::Saw,
            3 => LfoShape::Square,
            4 => LfoShape::Random,
            _ => LfoShape::Sine,
        }
    }
}

/// LFO parameters.
#[derive(Clone, Copy, Debug)]
pub struct LfoParams {
    /// Rate in Hz (0.01 to 20.0)
    pub rate: f32,
    /// Waveform shape (0-4)
    pub shape: u8,
    /// 0 = free-running, 1 = retrigger on note-on
    pub sync: u8,
    /// Start phase when retriggered (0.0 to 1.0)
    pub phase_offset: f32,
    /// Output depth multiplier (0.0 to 1.0)
    pub depth: f32,
    /// DC offset (-1.0 to 1.0) — shifts the output range
    pub offset: f32,
}

impl Default for LfoParams {
    fn default() -> Self {
        Self {
            rate: 1.0,       // 1 Hz
            shape: 0,        // sine
            sync: 0,         // free-running
            phase_offset: 0.0,
            depth: 1.0,      // full depth
            offset: 0.0,     // centered (bipolar)
        }
    }
}

impl LfoParams {
    pub const RATE: ParamId = ParamId(0);
    pub const SHAPE: ParamId = ParamId(1);
    pub const SYNC: ParamId = ParamId(2);
    pub const PHASE: ParamId = ParamId(3);
    pub const DEPTH: ParamId = ParamId(4);
    pub const OFFSET: ParamId = ParamId(5);
}

/// The LFO source is computed from the unmodulated `params.lfo`; modulating
/// the LFO itself is out of scope, so nothing here is modulatable.
pub static LFO_SPECS: [ParamSpec; 6] = [
    ParamSpec::continuous(0, "RATE", ValFmt::Uni, 0.01, 20.0, 1.0, 0.15, false),
    ParamSpec::choice(1, "SHAPE", ValFmt::Int(4), 4.0, 0.0),
    ParamSpec::choice(2, "SYNC", ValFmt::Int(1), 1.0, 0.0),
    ParamSpec::continuous(3, "PHASE", ValFmt::Uni, 0.0, 1.0, 0.0, 1.0 / 128.0, false),
    ParamSpec::continuous(4, "DEPTH", ValFmt::Uni, 0.0, 1.0, 1.0, 1.0 / 128.0, false),
    ParamSpec::continuous(5, "OFST", ValFmt::Bi, -1.0, 1.0, 0.0, 2.0 / 128.0, false),
];

impl Block for LfoParams {
    fn specs(&self) -> &'static [ParamSpec] {
        &LFO_SPECS
    }

    fn get(&self, id: ParamId) -> f32 {
        match id {
            Self::RATE => self.rate,
            Self::SHAPE => self.shape as f32,
            Self::SYNC => self.sync as f32,
            Self::PHASE => self.phase_offset,
            Self::DEPTH => self.depth,
            Self::OFFSET => self.offset,
            _ => 0.0,
        }
    }

    fn write(&mut self, id: ParamId, v: f32) {
        match id {
            Self::RATE => self.rate = v,
            Self::SHAPE => self.shape = v as u8,
            Self::SYNC => self.sync = v as u8,
            Self::PHASE => self.phase_offset = v,
            Self::DEPTH => self.depth = v,
            Self::OFFSET => self.offset = v,
            _ => {}
        }
    }
}

/// LFO state.
#[derive(Clone, Debug)]
pub struct Lfo {
    /// Phase accumulator (0.0 to 1.0)
    phase: f32,
    /// Current output value
    output: f32,
    /// Random/S&H: held value until next cycle
    random_value: f32,
    /// Simple PRNG state for random
    rng_state: u32,
}

impl Default for Lfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Lfo {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            output: 0.0,
            random_value: 0.0,
            rng_state: 12345,
        }
    }

    /// Reset phase (called on note-on if sync mode)
    pub fn retrigger(&mut self, phase_offset: f32) {
        self.phase = phase_offset;
    }

    /// Get current output without advancing
    pub fn current(&self) -> f32 {
        self.output
    }

    /// Advance LFO by one block and return the output value.
    /// Called once per render block (not per sample).
    pub fn process(&mut self, params: &LfoParams, sample_rate: u32) -> f32 {
        let shape = LfoShape::from_u8(params.shape);

        // Compute raw waveform from phase (0.0 to 1.0)
        let raw = match shape {
            LfoShape::Sine => {
                fast_sin(self.phase * core::f32::consts::TAU)
            }
            LfoShape::Triangle => {
                // 0→1→0→-1→0 over one cycle
                let t = self.phase;
                if t < 0.25 {
                    t * 4.0
                } else if t < 0.75 {
                    2.0 - t * 4.0
                } else {
                    t * 4.0 - 4.0
                }
            }
            LfoShape::Saw => {
                // -1 to +1 ramp
                self.phase * 2.0 - 1.0
            }
            LfoShape::Square => {
                if self.phase < 0.5 { 1.0 } else { -1.0 }
            }
            LfoShape::Random => {
                self.random_value // held until phase wraps
            }
        };

        // Apply depth and offset
        self.output = raw * params.depth + params.offset;
        self.output = self.output.clamp(-1.0, 1.0);

        // Advance phase
        let phase_inc = params.rate / sample_rate as f32;
        // LFO runs at block rate, so multiply by block size
        self.phase += phase_inc * chimera_hal::BLOCK_SIZE as f32;

        // Wrap phase
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            // Random: generate new value on wrap
            if shape == LfoShape::Random {
                self.rng_state = self.rng_state.wrapping_mul(1103515245).wrapping_add(12345);
                self.random_value = (self.rng_state >> 16) as f32 / 32768.0 - 1.0;
            }
        }

        self.output
    }
}
