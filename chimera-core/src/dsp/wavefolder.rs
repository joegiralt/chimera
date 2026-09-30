use crate::dsp::ease::{Gate, Ramp, at, step_of};
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
    mix: Ramp,
    gate: Gate,
}

impl Wavefolder {
    pub fn new() -> Self {
        Self::default()
    }

    /// A fresh note: no ramp from the last one's settings.
    pub fn hold(&mut self) {
        *self = Self::new();
    }

    /// Process a block of samples in-place; `on` is whether the stage runs
    /// (`Voice::stage_runs`), which its gate fades.
    pub fn process(&mut self, buf: &mut [f32], params: &FolderParams, on: bool) {
        let f = self.fold.step(params.fold);
        let y = self.sym.step(params.symmetry);
        let m = self.mix.step(params.mix);
        let g = self.gate.step(on);
        if g == (0.0, 0.0) {
            return;
        }
        if f.0 == f.1 && y.0 == y.1 && m.0 == m.1 && g == (1.0, 1.0) {
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
        let (sf, sy, sm, sg) = (step_of(f, n), step_of(y, n), step_of(m, n), step_of(g, n));
        // Silence's fold at the block's ends, lerped between: one fold a
        // sample, not two.
        let rest = |fold: f32, sym: f32| fold_wave((sym - 0.5) * 0.5 * (1.0 + fold * fold * 6.0));
        let r = (rest(f.0, y.0), rest(f.1, y.1));
        let sr = step_of(r, n);
        for (i, sample) in buf.iter_mut().enumerate() {
            let (fold, sym) = (at(f.0, sf, i), at(y.0, sy, i));
            let mix = at(m.0, sm, i) * at(g.0, sg, i);
            let dry = *sample;
            let (bias, gain) = ((sym - 0.5) * 0.5, 1.0 + fold * fold * 6.0);
            let folded = fold_wave((dry + bias) * gain) - at(r.0, sr, i);
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
            f.process(&mut buf, &p, true);
            assert!(buf.iter().all(|&x| x.abs() < 1e-6), "SYM {symmetry}");
        }
    }
}
