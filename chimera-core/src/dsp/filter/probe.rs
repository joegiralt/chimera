//! Static-cost probes (spec § Cost): each runs one block of one path,
//! `#[inline(never)]`, so `tools/loop_count.py` finds its loops by name in the
//! bench ELF. The bench's `FILTER_PROBES` keeps them linked.

use super::SvfFilter;
use crate::hw::{BLOCK_SIZE, SAMPLE_RATE};
use crate::params::FilterParams;

/// Today's LP24, the baseline every bill subtracts: `SvfFilter::process`'s
/// prelude with the mode fixed, so only LP24's two loops are linked. Cutoff
/// and DRIVE unchanged take the steady loop; changed, the ramped one.
#[inline(never)]
pub fn old_lp24_probe(f: &mut SvfFilter, buf: &mut [f32; BLOCK_SIZE], p: &FilterParams) {
    let sr = SAMPLE_RATE as f32;
    let fc = p.cutoff.min(sr * 0.49);
    let g = crate::dsp::fast_tan(core::f32::consts::PI * fc / sr);
    let from = f.g.replace(g).unwrap_or(g);
    let k = 2.0 * (1.0 - p.resonance) + 0.01;
    let pre = 1.0 + p.drive * 4.0;
    let pre = (f.pre.replace(pre).unwrap_or(pre), pre);
    f.run::<2>(buf, pre, (from, g), k);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filter::FilterMode;

    /// The probe is LP24's `process`, sample for sample: steady, ramped, and
    /// ramped to a cutoff past 0.49·fs.
    #[test]
    fn old_lp24_probe_is_process_at_lp24() {
        let mut p = FilterParams::default();
        (p.cutoff, p.resonance, p.drive) = (800.0, 0.7, 0.3);
        assert!(p.set_mode(FilterMode::Lp24));
        let (mut a, mut b) = (SvfFilter::new(), SvfFilter::new());
        let mut x = 0.37f32;
        for block in 0..5 {
            if block == 2 {
                p.cutoff = 3000.0;
                p.drive = 0.6;
            }
            if block == 4 {
                // Past 0.49·fs: the clamp.
                p.cutoff = SAMPLE_RATE as f32 * 0.6;
            }
            let mut want = [0.0f32; BLOCK_SIZE];
            for s in want.iter_mut() {
                x = crate::dsp::clampf(x * 3.9 * (1.0 - x), 0.01, 0.99);
                *s = x - 0.5;
            }
            let mut got = want;
            a.process(&mut want, &p, SAMPLE_RATE);
            old_lp24_probe(&mut b, &mut got, &p);
            assert_eq!(got, want, "block {block}");
        }
    }
}
