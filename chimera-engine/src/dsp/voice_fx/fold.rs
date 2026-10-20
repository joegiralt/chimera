//! FOLD: the triangle wavefolder, today's moved whole (ADR 0155). P1 is
//! FOLD, P2 SYM (its bias shapes the fold, not the level: silence folds to
//! silence, ADR 0060), P3 LEVEL on the wet.

use super::Ends;
use crate::dsp::clampf;
use crate::dsp::ease::{at, step_of};
use crate::dsp::filter::{SUB, step_to};
use crate::hw::BLOCK_SIZE;
use crate::taps::TapLane;

/// The wet's make-up gain: at FOLD 0.4, SYM and LEVEL ½ the wet is as loud
/// as the dry, within 1.5 dB (VA INIT, C3, INPUT ½). The fold ran 3.3 dB hot.
const MAKE_UP: f32 = 0.684;

/// The memoryless transfer at MIX 1, for the viz.
pub fn curve(x: f32, pots: [f32; 3]) -> f32 {
    let [fold, sym, level] = pots;
    let (bias, gain) = ((sym - 0.5) * 0.5, 1.0 + fold * fold * 6.0);
    (fold_wave((x + bias) * gain) - fold_wave(bias * gain))
        * (super::params::pot_level(level) * MAKE_UP)
}

/// `buf` through the stage. A FOLD tap moves FOLD per sample:
/// `clamp(fold + tap, 0, 1)`, silence's fold with it. `glide` steps an
/// untapped moving block `STEPS` times (ADR 0117).
pub fn run(buf: &mut [f32; BLOCK_SIZE], e: &Ends, glide: bool, lane: Option<&TapLane>) {
    let (f, y, m, g) = (e.pots[0], e.pots[1], e.mix, e.gate);
    if g == (0.0, 0.0) {
        return;
    }
    let l = e.level();
    let l = (l.0 * MAKE_UP, l.1 * MAKE_UP);
    let n = buf.len();
    if let Some(t) = lane {
        let (sf, sy, sm, sg, sl) = (
            step_of(f, n),
            step_of(y, n),
            step_of(m, n),
            step_of(g, n),
            step_of(l, n),
        );
        for (i, sample) in buf.iter_mut().enumerate() {
            let fold = clampf(at(f.0, sf, i) + t.at(i), 0.0, 1.0);
            let bias = (at(y.0, sy, i) - 0.5) * 0.5;
            let mix = at(m.0, sm, i) * at(g.0, sg, i);
            let gain = 1.0 + fold * fold * 6.0;
            let dry = *sample;
            let folded = (fold_wave((dry + bias) * gain) - fold_wave(bias * gain)) * at(l.0, sl, i);
            *sample = dry * (1.0 - mix) + folded * mix;
        }
        return;
    }
    if e.steady() {
        let (fold, sym, mix) = (f.1, y.1, m.1); // SYM 0.5: no bias
        let gain = 1.0 + fold * fold * 6.0; // quadratic gain ramp
        let bias = (sym - 0.5) * 0.5;
        let rest = fold_wave(bias * gain);
        for sample in buf.iter_mut() {
            let dry = *sample;
            let folded = (fold_wave((dry + bias) * gain) - rest) * l.1;
            *sample = dry * (1.0 - mix) + folded * mix;
        }
        return;
    }
    if g == (1.0, 1.0) && glide {
        for (j, sub) in buf.as_chunks_mut::<SUB>().0.iter_mut().enumerate() {
            let (fold, sym, mix) = (step_to(f, j), step_to(y, j), step_to(m, j));
            let gain = 1.0 + fold * fold * 6.0;
            let bias = (sym - 0.5) * 0.5;
            let rest = fold_wave(bias * gain);
            let level = step_to(l, j);
            for sample in sub {
                let dry = *sample;
                let folded = (fold_wave((dry + bias) * gain) - rest) * level;
                *sample = dry * (1.0 - mix) + folded * mix;
            }
        }
        return;
    }
    let (sf, sy, sm, sg, sl) = (
        step_of(f, n),
        step_of(y, n),
        step_of(m, n),
        step_of(g, n),
        step_of(l, n),
    );
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
        let folded = (fold_wave((dry + bias) * gain) - at(r.0, sr, i)) * at(l.0, sl, i);
        *sample = dry * (1.0 - mix) + folded * mix;
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
    extern crate std;
    use super::super::rig::{FNV0, Rig, fnv, sine};
    use super::*;

    fn bits(b: &[f32]) -> std::vec::Vec<u32> {
        b.iter().map(|x| x.to_bits()).collect()
    }

    fn lane_rows() -> [[f32; BLOCK_SIZE]; 2] {
        [sine(0, 1.0, 300.0), sine(0, 1.0, 70.0)]
    }

    /// The fold's cases through FOLD, FNV-1a over every block. Recorded
    /// against `Wavefolder` bit for bit before wavefolder.rs went (#614);
    /// re-recorded in Task 14 for `MAKE_UP` 0.684, the one intended change
    /// (no legacy compat): the wet is level-matched to the dry.
    const FOLD_STEADY: u64 = 0x6f65_c4f4_5c4e_a72b;
    const FOLD_MOVING: u64 = 0xb98d_394f_c321_7fd8;

    #[test]
    fn fold_at_level_half_is_todays_folder_bit_for_bit() {
        let rows = lane_rows();
        let lane = TapLane {
            rows: &rows,
            from: [0.1, 0.0],
            to: [0.3, 0.0],
        };
        let mut h = FNV0;
        for tapped in [false, true] {
            for fold in [0.01, 0.5, 1.0] {
                for sym in [0.0, 0.5, 1.0] {
                    for mix in [0.3, 1.0] {
                        let mut rig = Rig::default();
                        for b in 0..40 {
                            let mut n = sine(b, 0.7, 110.0);
                            let l = tapped.then_some(&lane);
                            run(&mut n, &rig.ends([fold, sym, 0.5], mix, true), false, l);
                            h = fnv(h, &n);
                        }
                    }
                }
            }
        }
        assert_eq!(h, FOLD_STEADY);
    }

    #[test]
    fn fold_moving_blocks_match_with_and_without_glide() {
        let mut h = FNV0;
        for glide in [false, true] {
            let mut rig = Rig::default();
            for b in 0..6 {
                let fold = if b < 2 { 0.2 } else { 0.8 };
                let mut n = sine(b, 0.7, 110.0);
                run(&mut n, &rig.ends([fold, 0.3, 0.5], 0.7, true), glide, None);
                h = fnv(h, &n);
            }
        }
        assert_eq!(h, FOLD_MOVING);
    }

    #[test]
    fn level_scales_only_the_wet() {
        let rows = lane_rows();
        let lane = TapLane {
            rows: &rows,
            from: [0.1, 0.0],
            to: [0.3, 0.0],
        };
        for l in [None, Some(&lane)] {
            for p3 in [0.0, 1.0] {
                let mut rig = Rig::default();
                let mut buf = sine(0, 0.7, 110.0);
                let dry = buf;
                run(&mut buf, &rig.ends([0.6, 0.3, p3], 0.0, true), false, l);
                assert_eq!(bits(&buf), bits(&dry), "P3 {p3}");
            }
        }
        let (mut lo, mut hi) = (Rig::default(), Rig::default());
        let (mut a, mut b) = (sine(0, 0.3, 110.0), sine(0, 0.3, 110.0));
        run(&mut a, &lo.ends([0.0, 0.5, 0.0], 1.0, true), false, None);
        run(&mut b, &hi.ends([0.0, 0.5, 1.0], 1.0, true), false, None);
        assert!(b[40].abs() > 4.0 * a[40].abs());
    }

    #[test]
    fn curve_is_the_kernel_at_mix_one() {
        let mut rig = Rig::default();
        let mut buf = sine(0, 0.9, 200.0);
        let x = buf;
        run(&mut buf, &rig.ends([0.4, 0.2, 0.7], 1.0, true), false, None);
        for (o, i) in buf.iter().zip(x) {
            assert_eq!(*o, curve(i, [0.4, 0.2, 0.7]));
        }
    }

    #[test]
    fn silence_folds_to_silence() {
        for sym in [0.0, 0.3, 0.5, 1.0] {
            let mut rig = Rig::default();
            let mut buf = [0.0f32; BLOCK_SIZE];
            run(&mut buf, &rig.ends([0.5, sym, 0.9], 1.0, true), false, None);
            assert!(buf.iter().all(|&x| x.abs() < 1e-6), "SYM {sym}");
        }
    }
}
