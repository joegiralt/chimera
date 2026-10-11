use crate::dsp::clampf;
use crate::dsp::ease::{Gate, Ramp, at, step_of};
use crate::dsp::filter::{SUB, step_to};
use crate::hw::BLOCK_SIZE;
use crate::params::FolderParams;
use crate::taps::TapLane;

/// Post-filter wavefolder.
/// Folds the signal by reflecting it at ±1 boundaries. SYM's bias, added
/// after the gain and ramped in with FOLD, shapes the fold, not the level:
/// silence folds to silence. FOLD, SYM and MIX ramp across each block;
/// below FOLD 0.001 the gate fades the fold out over 20 ms, then rests.
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
    /// (`Voice::stage_runs`), which its gate fades. A FOLD tap moves FOLD
    /// per sample: `clamp(fold + tap, 0, 1)`, silence's fold with it.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        params: &FolderParams,
        on: bool,
        tap: Option<&TapLane>,
    ) {
        self.process_glide(buf, params, on, tap, || false);
    }

    /// `process`; with `glide` (only a knob moves a setting) an untapped
    /// moving block steps `STEPS` times, each sub-block the steady loop
    /// (ADR 0117). It's asked only of a moving block.
    pub fn process_glide(
        &mut self,
        buf: &mut [f32],
        params: &FolderParams,
        on: bool,
        tap: Option<&TapLane>,
        glide: impl FnOnce() -> bool,
    ) {
        let f = self.fold.step(params.fold);
        let y = self.sym.step(params.symmetry);
        let m = self.mix.step(params.mix);
        let g = self.gate.step(on);
        if g == (0.0, 0.0) {
            return;
        }
        // The voice's blocks are `BLOCK_SIZE`, as the tap's rows are.
        if let (Some(t), Ok(b)) = (tap, <&mut [f32; BLOCK_SIZE]>::try_from(&mut *buf)) {
            return Self::tapped(b, (f, y, m, g), t);
        }
        if f.0 == f.1 && y.0 == y.1 && m.0 == m.1 && g == (1.0, 1.0) {
            let (fold, sym, mix) = (f.1, y.1, m.1); // SYM 0.5: no bias
            let gain = 1.0 + fold * fold * 6.0; // quadratic gain ramp
            let bias = sym_bias(sym, fold);
            let rest = fold_wave(bias);
            for sample in buf.iter_mut() {
                let dry = *sample;
                let folded = fold_wave(dry * gain + bias) - rest;
                *sample = dry * (1.0 - mix) + folded * mix;
            }
            return;
        }
        if g == (1.0, 1.0) && buf.len() == BLOCK_SIZE && glide() {
            for (j, sub) in buf.as_chunks_mut::<SUB>().0.iter_mut().enumerate() {
                let (fold, sym, mix) = (step_to(f, j), step_to(y, j), step_to(m, j));
                let gain = 1.0 + fold * fold * 6.0;
                let bias = sym_bias(sym, fold);
                let rest = fold_wave(bias);
                for sample in sub {
                    let dry = *sample;
                    let folded = fold_wave(dry * gain + bias) - rest;
                    *sample = dry * (1.0 - mix) + folded * mix;
                }
            }
            return;
        }
        let n = buf.len();
        let (sf, sy, sm, sg) = (step_of(f, n), step_of(y, n), step_of(m, n), step_of(g, n));
        // Silence's fold at the block's ends, lerped between: one fold a
        // sample, not two.
        let rest = |fold: f32, sym: f32| fold_wave(sym_bias(sym, fold));
        let r = (rest(f.0, y.0), rest(f.1, y.1));
        let sr = step_of(r, n);
        for (i, sample) in buf.iter_mut().enumerate() {
            let (fold, sym) = (at(f.0, sf, i), at(y.0, sy, i));
            let mix = at(m.0, sm, i) * at(g.0, sg, i);
            let dry = *sample;
            let (bias, gain) = (sym_bias(sym, fold), 1.0 + fold * fold * 6.0);
            let folded = fold_wave(dry * gain + bias) - at(r.0, sr, i);
            *sample = dry * (1.0 - mix) + folded * mix;
        }
    }
}

impl Wavefolder {
    /// The per-sample path with FOLD tapped: silence's fold is taken each
    /// sample, since FOLD moves within it.
    fn tapped(buf: &mut [f32; BLOCK_SIZE], r: Ends, t: &TapLane) {
        let (f, y, m, g) = r;
        let n = buf.len();
        let (sf, sy, sm, sg) = (step_of(f, n), step_of(y, n), step_of(m, n), step_of(g, n));
        for (i, sample) in buf.iter_mut().enumerate() {
            let fold = clampf(at(f.0, sf, i) + t.at(i), 0.0, 1.0);
            let mix = at(m.0, sm, i) * at(g.0, sg, i);
            let bias = sym_bias(at(y.0, sy, i), fold);
            let gain = 1.0 + fold * fold * 6.0;
            let dry = *sample;
            let folded = fold_wave(dry * gain + bias) - fold_wave(bias);
            *sample = dry * (1.0 - mix) + folded * mix;
        }
    }
}

/// FOLD's, SYM's, MIX's and the gate's `(from, to)` for a block.
type Ends = ((f32, f32), (f32, f32), (f32, f32), (f32, f32));

/// SYM's bias span, post-gain: |bias| <= 0.9 stays off the fold point (1).
const SYM_SPAN: f32 = 1.8;
/// FOLD at which SYM's bias is fully in; below it the bias ramps in.
const SYM_RAMP: f32 = 0.25;

/// SYM's bias in post-gain units, ramped in with FOLD. It moves neither
/// with the gain (no sign flip as FOLD turns) nor the level (`fold_wave`
/// is 1-Lipschitz).
fn sym_bias(sym: f32, fold: f32) -> f32 {
    (sym - 0.5) * SYM_SPAN * (fold / SYM_RAMP).min(1.0)
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
            f.process(&mut buf, &p, true, None);
            assert!(buf.iter().all(|&x| x.abs() < 1e-6), "SYM {symmetry}");
        }
    }
}
