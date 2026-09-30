use crate::dsp::ease::{Ramp, at, step_of};
use crate::params::DriveParams;

/// Pre-filter saturation stage.
/// Symmetric soft clipping (`fast_tanh`) with a tone control and dry/wet mix.
/// DRIVE, TONE and MIX ramp across each block; below DRIVE 0.001 the stage
/// fades its wet out over a block, then rests.
#[derive(Clone, Copy, Debug, Default)]
pub struct Drive {
    drive: Ramp,
    tone: Ramp,
    /// MIX × on.
    mix: Ramp,
}

impl Drive {
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh note: no ramp from the last one's settings.
    pub fn hold(&mut self) {
        *self = Self::new();
    }

    /// Process a block of samples in-place.
    pub fn process(&mut self, buf: &mut [f32], params: &DriveParams) {
        let on = if params.drive < 0.001 { 0.0 } else { 1.0 };
        let d = self.drive.step(params.drive);
        let t = self.tone.step(params.tone);
        let m = self.mix.step(params.mix * on);
        if m == (0.0, 0.0) {
            return;
        }
        if d.0 == d.1 && t.0 == t.1 && m.0 == m.1 {
            let gain = 1.0 + d.1 * 8.0; // 1x to 9x input gain
            // Tone: no filter. The pre- and post-clip gains move opposite ways:
            // below 0.5 the clip is softer (fewer harmonics), above it harder.
            let (pre_bright, post_dark, mix) = (0.5 + t.1, 1.5 - t.1, m.1);
            for sample in buf.iter_mut() {
                let dry = *sample;
                let clipped = crate::dsp::fast_tanh(dry * gain * pre_bright) * post_dark;
                *sample = dry * (1.0 - mix) + clipped * mix;
            }
            return;
        }
        let n = buf.len();
        let (sd, st, sm) = (step_of(d, n), step_of(t, n), step_of(m, n));
        for (i, sample) in buf.iter_mut().enumerate() {
            let (drive, tone, mix) = (at(d.0, sd, i), at(t.0, st, i), at(m.0, sm, i));
            let dry = *sample;
            let driven = dry * (1.0 + drive * 8.0) * (0.5 + tone);
            let clipped = crate::dsp::fast_tanh(driven) * (1.5 - tone);
            *sample = dry * (1.0 - mix) + clipped * mix;
        }
    }
}
