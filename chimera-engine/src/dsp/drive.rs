use crate::dsp::ease::{Gate, Ramp, at, step_of};
use crate::params::DriveParams;

/// Pre-filter saturation stage.
/// Symmetric soft clipping (`fast_tanh`) with a tone control and dry/wet mix.
/// DRIVE, TONE and MIX ramp across each block; switched off, the stage
/// fades its wet out over its gate's 20 ms, then rests.
#[derive(Clone, Copy, Debug, Default)]
pub struct Drive {
    drive: Ramp,
    tone: Ramp,
    mix: Ramp,
    gate: Gate,
}

impl Drive {
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh note: no ramp from the last one's settings.
    pub fn hold(&mut self) {
        *self = Self::new();
    }

    /// Process a block of samples in-place; `on` is whether the stage runs
    /// (`Voice::stage_runs`), which its gate fades.
    pub fn process(&mut self, buf: &mut [f32], params: &DriveParams, on: bool) {
        let d = self.drive.step(params.drive);
        let t = self.tone.step(params.tone);
        let m = self.mix.step(params.mix);
        let g = self.gate.step(on);
        if g == (0.0, 0.0) {
            return;
        }
        if d.0 == d.1 && t.0 == t.1 && m.0 == m.1 && g == (1.0, 1.0) {
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
        let (sd, st, sm, sg) = (step_of(d, n), step_of(t, n), step_of(m, n), step_of(g, n));
        for (i, sample) in buf.iter_mut().enumerate() {
            let (drive, tone) = (at(d.0, sd, i), at(t.0, st, i));
            let mix = at(m.0, sm, i) * at(g.0, sg, i);
            let dry = *sample;
            let driven = dry * (1.0 + drive * 8.0) * (0.5 + tone);
            let clipped = crate::dsp::fast_tanh(driven) * (1.5 - tone);
            *sample = dry * (1.0 - mix) + clipped * mix;
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;

    /// The gate fades over 20 ms: at TONE 0 or 1, where DRIVE's wet at
    /// 0.001 is 2.5 dB under the dry, switching off steps no sine's slope.
    #[test]
    fn the_gate_never_clicks_at_any_tone() {
        for tone in [0.0, 0.5, 1.0] {
            let p = DriveParams {
                drive: 0.0011,
                tone,
                mix: 1.0,
            };
            let mut d = Drive::new();
            let mut out = std::vec::Vec::new();
            for b in 0..70 {
                let mut buf: [f32; 64] = core::array::from_fn(|i| {
                    let t = (b * 64 + i) as f32 / 48_000.0;
                    0.5 * libm::sinf(core::f32::consts::TAU * 55.0 * t)
                });
                d.process(&mut buf, &p, b < 40);
                out.extend_from_slice(&buf);
            }
            let d2 = |a: usize, b: usize| {
                out[a * 64..b * 64]
                    .windows(3)
                    .map(|w| (w[2] - 2.0 * w[1] + w[0]).abs())
                    .fold(0.0f32, f32::max)
            };
            let (steady, switch) = (d2(20, 40).max(d2(55, 70)), d2(40, 55));
            assert!(switch < 1.5 * steady, "TONE {tone}: {switch} vs {steady}");
        }
    }
}
