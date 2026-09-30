use crate::dsp::ease::{Ramp, at, step_of};
use crate::params::FolderParams;

/// Post-filter wavefolder.
/// Folds the signal by reflecting it at ±1 boundaries. SYM's bias shapes
/// the fold, not the level: silence folds to silence. FOLD, SYM and MIX
/// ramp across each block; below FOLD 0.001 the fold fades out over a
/// block, then rests.
#[derive(Clone, Copy, Debug, Default)]
pub struct Wavefolder {
    fold: Ramp,
    sym: Ramp,
    /// MIX × on.
    mix: Ramp,
}

impl Wavefolder {
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh note: no ramp from the last one's settings.
    pub fn hold(&mut self) {
        *self = Self::new();
    }

    /// Process a block of samples in-place.
    pub fn process(&mut self, buf: &mut [f32], params: &FolderParams) {
        let on = if params.fold < 0.001 { 0.0 } else { 1.0 };
        let f = self.fold.step(params.fold);
        let y = self.sym.step(params.symmetry);
        let m = self.mix.step(params.mix * on);
        if m == (0.0, 0.0) {
            return;
        }
        if f.0 == f.1 && y.0 == y.1 && m.0 == m.1 {
            let (fold, sym, mix) = (f.1, y.1, m.1); // SYM 0.5: no bias
            let gain = 1.0 + fold * fold * 6.0; // quadratic gain ramp
            let bias = (sym - 0.5) * 0.5;
            let rest = fold_wave(bias * gain);
            for sample in buf.iter_mut() {
                let dry = *sample;
                let folded = fold_wave((dry + bias) * gain) - rest;
                *sample = dry * (1.0 - mix) + folded * mix;
            }
            return;
        }
        let n = buf.len();
        let (sf, sy, sm) = (step_of(f, n), step_of(y, n), step_of(m, n));
        for (i, sample) in buf.iter_mut().enumerate() {
            let (fold, sym, mix) = (at(f.0, sf, i), at(y.0, sy, i), at(m.0, sm, i));
            let dry = *sample;
            let (bias, gain) = ((sym - 0.5) * 0.5, 1.0 + fold * fold * 6.0);
            let folded = fold_wave((dry + bias) * gain) - fold_wave(bias * gain);
            *sample = dry * (1.0 - mix) + folded * mix;
        }
    }
}

/// Triangle-fold: reflects signal at ±1 boundaries, keeping output in -1..1.
fn fold_wave(x: f32) -> f32 {
    let x = x + 1.0;
    let period = 4.0;
    let d = x / period;
    let t = x
        - (if d >= 0.0 {
            d as i32 as f32
        } else {
            (d as i32 - 1) as f32
        }) * period;
    if t < 2.0 { t - 1.0 } else { 3.0 - t }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SYM biases the fold, not the level: silence folds to silence at any
    /// SYM, so no offset outlives a note.
    #[test]
    fn silence_folds_to_silence() {
        for symmetry in [0.0, 0.3, 0.5, 1.0] {
            let p = FolderParams {
                fold: 0.5,
                symmetry,
                mix: 1.0,
            };
            let mut f = Wavefolder::new();
            let mut buf = [0.0f32; 64];
            f.process(&mut buf, &p);
            assert!(buf.iter().all(|&x| x.abs() < 1e-6), "SYM {symmetry}");
        }
    }
}
