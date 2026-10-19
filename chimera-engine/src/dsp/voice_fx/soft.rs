//! SOFT: symmetric soft clipping (`fast_tanh`) with a tone control, today's
//! DRV moved whole (ADR 0155). P1 is DRIVE, P2 TONE, P3 LEVEL on the wet.

use super::Ends;
use crate::dsp::ease::{at, step_of};
use crate::dsp::fast_tanh;
use crate::dsp::filter::{SUB, step_to};
use crate::hw::BLOCK_SIZE;

/// The memoryless transfer at MIX 1, for the viz.
pub fn curve(x: f32, pots: [f32; 3]) -> f32 {
    let [drive, tone, level] = pots;
    fast_tanh(x * (1.0 + drive * 8.0) * (0.5 + tone))
        * (1.5 - tone)
        * super::params::pot_level(level)
}

/// `buf` through the stage. `glide` (only a knob moves a setting) steps a
/// moving block `STEPS` times, each sub-block the steady loop (ADR 0117).
pub fn run(buf: &mut [f32; BLOCK_SIZE], e: &Ends, glide: bool) {
    let (d, t, m, g) = (e.pots[0], e.pots[1], e.mix, e.gate);
    if g == (0.0, 0.0) {
        return;
    }
    let l = e.level();
    if e.steady() {
        let gain = 1.0 + d.1 * 8.0; // 1x to 9x input gain
        // Tone: no filter. The pre- and post-clip gains move opposite ways:
        // below 0.5 the clip is softer (fewer harmonics), above it harder.
        let (pre_bright, post_dark, mix) = (0.5 + t.1, 1.5 - t.1, m.1);
        for sample in buf.iter_mut() {
            let dry = *sample;
            let clipped = fast_tanh(dry * gain * pre_bright) * post_dark * l.1;
            *sample = dry * (1.0 - mix) + clipped * mix;
        }
        return;
    }
    if g == (1.0, 1.0) && glide {
        for (j, sub) in buf.as_chunks_mut::<SUB>().0.iter_mut().enumerate() {
            let gain = 1.0 + step_to(d, j) * 8.0;
            let tone = step_to(t, j);
            let (pre_bright, post_dark, mix) = (0.5 + tone, 1.5 - tone, step_to(m, j));
            let level = step_to(l, j);
            for sample in sub {
                let dry = *sample;
                let clipped = fast_tanh(dry * gain * pre_bright) * post_dark * level;
                *sample = dry * (1.0 - mix) + clipped * mix;
            }
        }
        return;
    }
    let n = buf.len();
    let (sd, st, sm, sg, sl) = (
        step_of(d, n),
        step_of(t, n),
        step_of(m, n),
        step_of(g, n),
        step_of(l, n),
    );
    for (i, sample) in buf.iter_mut().enumerate() {
        let (drive, tone) = (at(d.0, sd, i), at(t.0, st, i));
        let mix = at(m.0, sm, i) * at(g.0, sg, i);
        let dry = *sample;
        let driven = dry * (1.0 + drive * 8.0) * (0.5 + tone);
        let clipped = fast_tanh(driven) * (1.5 - tone) * at(l.0, sl, i);
        *sample = dry * (1.0 - mix) + clipped * mix;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::super::rig::{FNV0, Rig, fnv, sine};
    use super::*;

    fn bits(b: &[f32]) -> std::vec::Vec<u32> {
        b.iter().map(|x| x.to_bits()).collect()
    }

    /// Today's DRV on these cases, FNV-1a over every block, recorded
    /// against `Drive` bit for bit before drive.rs went (#614).
    const DRV_STEADY: u64 = 0xeaaf_d861_f95a_8657;
    const DRV_MOVING: u64 = 0x4764_583d_3241_498e;

    #[test]
    fn soft_at_level_half_is_todays_drive_bit_for_bit() {
        let mut h = FNV0;
        for drive in [0.01, 0.5, 1.0] {
            for tone in [0.0, 0.5, 1.0] {
                for mix in [0.3, 1.0] {
                    let mut rig = Rig::default();
                    for b in 0..40 {
                        let mut n = sine(b, 0.7, 110.0);
                        run(&mut n, &rig.ends([drive, tone, 0.5], mix, true), false);
                        h = fnv(h, &n);
                    }
                }
            }
        }
        assert_eq!(h, DRV_STEADY);
    }

    #[test]
    fn soft_moving_blocks_match_with_and_without_glide() {
        let mut h = FNV0;
        for glide in [false, true] {
            let mut rig = Rig::default();
            for b in 0..6 {
                let drive = if b < 2 { 0.2 } else { 0.8 };
                let mut n = sine(b, 0.7, 110.0);
                run(&mut n, &rig.ends([drive, 0.3, 0.5], 0.7, true), glide);
                h = fnv(h, &n);
            }
        }
        assert_eq!(h, DRV_MOVING);
    }

    #[test]
    fn level_scales_only_the_wet() {
        for p3 in [0.0, 1.0] {
            let mut rig = Rig::default();
            let mut buf = sine(0, 0.7, 110.0);
            let dry = buf;
            run(&mut buf, &rig.ends([0.6, 0.5, p3], 0.0, true), false);
            assert_eq!(bits(&buf), bits(&dry), "P3 {p3}");
        }
        let (mut lo, mut hi) = (Rig::default(), Rig::default());
        let (mut a, mut b) = (sine(0, 0.7, 110.0), sine(0, 0.7, 110.0));
        run(&mut a, &lo.ends([0.6, 0.5, 0.0], 1.0, true), false);
        run(&mut b, &hi.ends([0.6, 0.5, 1.0], 1.0, true), false);
        assert!(b[40].abs() > 4.0 * a[40].abs());
    }

    #[test]
    fn curve_is_the_kernel_at_mix_one() {
        let mut rig = Rig::default();
        let mut buf = sine(0, 0.9, 200.0);
        let x = buf;
        run(&mut buf, &rig.ends([0.4, 0.2, 0.7], 1.0, true), false);
        for (o, i) in buf.iter().zip(x) {
            assert_eq!(*o, curve(i, [0.4, 0.2, 0.7]));
        }
    }

    /// The gate fades over 20 ms: at TONE 0 or 1, where DRIVE's wet at
    /// 0.001 is 2.5 dB under the dry, switching off steps no sine's slope.
    #[test]
    fn the_gate_never_clicks_at_any_tone() {
        for tone in [0.0, 0.5, 1.0] {
            let mut rig = Rig::default();
            let mut out = std::vec::Vec::new();
            for b in 0..70 {
                let mut buf = sine(b, 0.5, 55.0);
                run(&mut buf, &rig.ends([0.0011, tone, 0.5], 1.0, b < 40), false);
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
