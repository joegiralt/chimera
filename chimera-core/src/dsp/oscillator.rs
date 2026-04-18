use core::f32::consts::PI;
use libm::sinf;

#[derive(Clone, Debug)]
pub struct SineOsc {
    phase: f32,
    phase_inc: f32,
}

impl SineOsc {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            phase_inc: 0.0,
        }
    }

    pub fn set_frequency(&mut self, freq: f32, sample_rate: u32) {
        self.phase_inc = freq / sample_rate as f32;
    }

    pub fn render(&mut self, output: &mut [f32]) {
        if self.phase_inc == 0.0 {
            for s in output.iter_mut() {
                *s = 0.0;
            }
            return;
        }
        for s in output.iter_mut() {
            *s = sinf(self.phase * 2.0 * PI);
            self.phase += self.phase_inc;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
        }
    }
}
