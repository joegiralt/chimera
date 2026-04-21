/// Pizza oscillator — variable-symmetry triangle with XOR bitcrush.
/// Inspired by the Bastl Pizza (ATtiny85 VCO).
///
/// Parameters:
/// - frequency: pitch in Hz
/// - shape: 0.0 = ramp down, 0.5 = triangle, 1.0 = ramp up
/// - crush: 0.0 = clean, 1.0 = fully bitcrushed
/// - level: output level 0.0..1.0

use chimera_hal::BLOCK_SIZE;

/// Pizza oscillator state.
#[derive(Clone, Debug)]
pub struct PizzaOsc {
    /// Phase accumulator (0.0..1.0)
    phase: f32,
    /// Phase increment per sample
    phase_inc: f32,
    /// Is the ramp going up?
    going_up: bool,
    /// Current ramp value (0.0..1.0)
    value: f32,
    /// Active flag
    active: bool,
}

/// Parameters for the Pizza oscillator.
#[derive(Clone, Copy, Debug)]
pub struct PizzaParams {
    /// Waveshape: 0.0 = ramp down, 0.5 = triangle, 1.0 = ramp up
    pub shape: f32,
    /// XOR bitcrush amount: 0.0 = clean, 1.0 = full crush
    pub crush: f32,
    /// Output level
    pub level: f32,
}

impl Default for PizzaParams {
    fn default() -> Self {
        Self {
            shape: 0.5,  // triangle
            crush: 0.0,  // clean
            level: 0.8,
        }
    }
}

impl Default for PizzaOsc {
    fn default() -> Self {
        Self::new()
    }
}

impl PizzaOsc {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            phase_inc: 0.0,
            going_up: true,
            value: 0.0,
            active: false,
        }
    }

    pub fn note_on(&mut self, freq: f32, sample_rate: u32) {
        self.phase_inc = freq / sample_rate as f32;
        self.phase = 0.0;
        self.going_up = true;
        self.value = 0.0;
        self.active = true;
    }

    pub fn note_off(&mut self) {
        // Let the envelope handle the release — we just keep oscillating
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Render a block of audio.
    pub fn render(
        &mut self,
        output: &mut [f32; BLOCK_SIZE],
        params: &PizzaParams,
        _sample_rate: u32,
    ) {
        if !self.active || self.phase_inc == 0.0 {
            for s in output.iter_mut() {
                *s = 0.0;
            }
            return;
        }

        // Shape controls the ratio of up/down ramp time
        // shape = 0.0 → all down (ramp down / reverse saw)
        // shape = 0.5 → equal up/down (triangle)
        // shape = 1.0 → all up (ramp up / sawtooth)
        let shape = params.shape.clamp(0.01, 0.99);
        let up_rate = 1.0 / shape;          // how fast we ramp up
        let down_rate = 1.0 / (1.0 - shape); // how fast we ramp down

        // Crush: convert to an 8-bit XOR mask
        let crush_bits = (params.crush * 255.0) as u8;

        let level = params.level;

        for s in output.iter_mut() {
            // Advance the ramp
            if self.going_up {
                self.value += self.phase_inc * up_rate;
                if self.value >= 1.0 {
                    self.value = 2.0 - self.value; // reflect
                    self.going_up = false;
                }
            } else {
                self.value -= self.phase_inc * down_rate;
                if self.value <= 0.0 {
                    self.value = -self.value; // reflect
                    self.going_up = true;
                }
            }

            // Convert to bipolar (-1..1)
            let mut sample = self.value * 2.0 - 1.0;

            // XOR bitcrush: quantize to 8-bit, apply mask, convert back
            if crush_bits > 0 {
                // Map -1..1 to 0..255
                let byte = ((sample * 0.5 + 0.5) * 255.0) as u8;
                // OR with crush mask (like the Bastl Pizza)
                let crushed = byte | crush_bits;
                // Back to -1..1
                sample = (crushed as f32 / 255.0) * 2.0 - 1.0;
            }

            *s = sample * level;
        }
    }
}
