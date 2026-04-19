//! Juno-style BBD chorus with binaural stereo output.
//!
//! The Roland Juno-60/106 chorus is a single BBD delay line modulated
//! by a triangle LFO. The magic is in the specific LFO rates and the
//! stereo trick: L = dry + wet, R = dry - wet (phase inversion creates
//! wide stereo image from a mono source).
//!
//! Mode I:  triangle LFO at 0.513 Hz, depth ~1.7ms
//! Mode II: triangle LFO at 0.863 Hz, depth ~2.3ms
//! Mode I+II: both LFOs running simultaneously (thickest)

use chimera_hal::BLOCK_SIZE;

const MAX_CHORUS_DELAY: usize = 2048; // ~42ms at 48kHz, plenty for chorus

/// Chorus mode selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ChorusMode {
    Off = 0,
    JunoI = 1,   // Slow, subtle
    JunoII = 2,  // Faster, wider
    JunoBoth = 3, // Both LFOs (thickest)
}

impl ChorusMode {
    pub fn from_u8(v: u8) -> Self {
        match v % 4 {
            0 => ChorusMode::Off,
            1 => ChorusMode::JunoI,
            2 => ChorusMode::JunoII,
            _ => ChorusMode::JunoBoth,
        }
    }
}

/// Chorus parameters.
#[derive(Clone, Copy, Debug)]
pub struct ChorusParams {
    /// Mode: 0=off, 1=Juno I, 2=Juno II, 3=Both
    pub mode: u8,
    /// Rate multiplier (0.5..2.0, centered at 1.0)
    pub rate: f32,
    /// Depth multiplier (0.5..2.0, centered at 1.0)
    pub depth: f32,
    /// Dry/wet mix (0..1)
    pub mix: f32,
}

impl Default for ChorusParams {
    fn default() -> Self {
        Self {
            mode: 0, // off by default
            rate: 0.5,
            depth: 0.5,
            mix: 0.0,
        }
    }
}

/// Single BBD delay line with triangle LFO.
struct BbdLine {
    buffer: [f32; MAX_CHORUS_DELAY],
    write_pos: usize,
    lfo_phase: f32,
}

impl BbdLine {
    fn new() -> Self {
        Self {
            buffer: [0.0; MAX_CHORUS_DELAY],
            write_pos: 0,
            lfo_phase: 0.0,
        }
    }

    /// Process one sample. Returns the modulated delayed sample.
    #[inline]
    fn tick(&mut self, input: f32, base_delay_ms: f32, lfo_rate_hz: f32, depth_ms: f32, sample_rate: u32) -> f32 {
        // Write input
        self.buffer[self.write_pos] = input;
        self.write_pos = (self.write_pos + 1) % MAX_CHORUS_DELAY;

        // Triangle LFO
        self.lfo_phase += lfo_rate_hz / sample_rate as f32;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= 1.0;
        }
        // Triangle: 0→1→0→-1→0
        let tri = if self.lfo_phase < 0.25 {
            self.lfo_phase * 4.0
        } else if self.lfo_phase < 0.75 {
            2.0 - self.lfo_phase * 4.0
        } else {
            self.lfo_phase * 4.0 - 4.0
        };

        // Modulated delay time
        let delay_samples = (base_delay_ms + tri * depth_ms) * sample_rate as f32 / 1000.0;
        let delay_samples = delay_samples.clamp(1.0, (MAX_CHORUS_DELAY - 2) as f32);

        // Interpolated read
        let d_int = delay_samples as usize;
        let d_frac = delay_samples - d_int as f32;
        let pos_a = (self.write_pos + MAX_CHORUS_DELAY - d_int) % MAX_CHORUS_DELAY;
        let pos_b = (self.write_pos + MAX_CHORUS_DELAY - d_int - 1) % MAX_CHORUS_DELAY;

        self.buffer[pos_a] * (1.0 - d_frac) + self.buffer[pos_b] * d_frac
    }
}

/// Juno-style binaural chorus.
/// Processes mono input, outputs mono (L+R summed).
/// For true stereo: L = dry + wet, R = dry - wet.
pub struct JunoChorus {
    line_i: BbdLine,
    line_ii: BbdLine,
}

impl Default for JunoChorus {
    fn default() -> Self {
        Self::new()
    }
}

impl JunoChorus {
    pub fn new() -> Self {
        Self {
            line_i: BbdLine::new(),
            line_ii: BbdLine::new(),
        }
    }

    pub fn process(&mut self, buf: &mut [f32; BLOCK_SIZE], params: &ChorusParams, sample_rate: u32) {
        let mode = ChorusMode::from_u8(params.mode);
        if mode == ChorusMode::Off || params.mix < 0.001 {
            return;
        }

        // Juno I: 0.513 Hz LFO, 1.7ms depth, 3.6ms base delay
        // Juno II: 0.863 Hz LFO, 2.3ms depth, 3.6ms base delay
        let rate_mult = 0.5 + params.rate * 1.5; // 0.5x to 2.0x
        let depth_mult = 0.5 + params.depth * 1.5;

        let base_delay = 3.6; // ms — Juno center delay

        for s in buf.iter_mut() {
            let dry = *s;
            let mut wet = 0.0;

            match mode {
                ChorusMode::JunoI => {
                    wet = self.line_i.tick(
                        dry,
                        base_delay,
                        0.513 * rate_mult,
                        1.7 * depth_mult,
                        sample_rate,
                    );
                }
                ChorusMode::JunoII => {
                    wet = self.line_ii.tick(
                        dry,
                        base_delay,
                        0.863 * rate_mult,
                        2.3 * depth_mult,
                        sample_rate,
                    );
                }
                ChorusMode::JunoBoth => {
                    let w1 = self.line_i.tick(
                        dry,
                        base_delay,
                        0.513 * rate_mult,
                        1.7 * depth_mult,
                        sample_rate,
                    );
                    let w2 = self.line_ii.tick(
                        dry,
                        base_delay,
                        0.863 * rate_mult,
                        2.3 * depth_mult,
                        sample_rate,
                    );
                    wet = (w1 + w2) * 0.5;
                }
                ChorusMode::Off => {}
            }

            // Binaural mono sum: (dry + wet) + (dry - wet) = 2*dry
            // For mono output: mix dry with wet directly
            // The binaural stereo magic happens when we have L/R outputs:
            //   L = dry + wet * mix
            //   R = dry - wet * mix (phase inversion = wide stereo)
            // For now, mono mix:
            *s = dry * (1.0 - params.mix * 0.5) + wet * params.mix;
        }
    }
}
