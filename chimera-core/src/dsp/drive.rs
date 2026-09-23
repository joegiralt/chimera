use crate::params::DriveParams;

/// Pre-filter saturation stage.
/// Asymmetric soft clipping via tanh with tone tilt and dry/wet mix.
pub struct Drive {
    // No state needed — purely memoryless waveshaping
}

impl Default for Drive {
    fn default() -> Self {
        Self::new()
    }
}

impl Drive {
    pub fn new() -> Self {
        Self {}
    }

    /// Process a block of samples in-place.
    pub fn process(&self, buf: &mut [f32], params: &DriveParams) {
        let drive = params.drive;
        if drive < 0.001 {
            // Drive at zero: apply only dry/wet mix (which at mix=1 is passthrough)
            return;
        }

        let gain = 1.0 + drive * 8.0; // 1x to 9x input gain
        let tone = params.tone; // 0=dark, 0.5=neutral, 1=bright
        let mix = params.mix; // 0..1 dry/wet

        // Tone: simple tilt EQ via asymmetric pre/post gain
        // tone < 0.5 = reduce highs (softer clip), tone > 0.5 = boost highs
        let pre_bright = 0.5 + tone;
        let post_dark = 1.5 - tone;

        for sample in buf.iter_mut() {
            let dry = *sample;

            // Apply gain + tone-dependent scaling
            let driven = dry * gain * pre_bright;

            // Soft clip via tanh
            let clipped = crate::dsp::fast_tanh(driven) * post_dark;

            // Dry/wet mix
            *sample = dry * (1.0 - mix) + clipped * mix;
        }
    }
}
