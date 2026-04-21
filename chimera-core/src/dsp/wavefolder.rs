use crate::params::FolderParams;

/// Post-filter wavefolder.
/// Folds the signal by reflecting it at ±1 boundaries.
pub struct Wavefolder {
    // Stateless — purely memoryless
}

impl Default for Wavefolder {
    fn default() -> Self {
        Self::new()
    }
}

impl Wavefolder {
    pub fn new() -> Self {
        Self {}
    }

    /// Process a block of samples in-place.
    pub fn process(&self, buf: &mut [f32], params: &FolderParams) {
        let fold = params.fold.value;
        if fold < 0.001 {
            return;
        }

        let sym = params.symmetry.value; // 0..1, center=0.5=no bias
        let mix = params.mix.value;
        let gain = 1.0 + fold * fold * 6.0; // quadratic gain ramp

        for sample in buf.iter_mut() {
            let dry = *sample;

            // Apply symmetry bias
            let biased = dry + (sym - 0.5) * 0.5;

            // Apply gain then fold
            let driven = biased * gain;
            let folded = fold_wave(driven);

            // Dry/wet
            *sample = dry * (1.0 - mix) + folded * mix;
        }
    }
}

/// Triangle-fold: reflects signal at ±1 boundaries, keeping output in -1..1.
fn fold_wave(x: f32) -> f32 {
    let x = x + 1.0;
    let period = 4.0;
    let d = x / period;
    let t = x - (if d >= 0.0 { d as i32 as f32 } else { (d as i32 - 1) as f32 }) * period;
    if t < 2.0 { t - 1.0 } else { 3.0 - t }
}
