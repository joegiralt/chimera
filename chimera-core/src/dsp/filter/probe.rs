//! Static-cost probes (spec § Cost): each runs one block of one path,
//! `#[inline(never)]`, so `tools/loop_count.py` finds its loops by name in the
//! bench ELF. The bench's `FILTER_PROBES` keeps them linked.
//!
//! Today's LP24 before CLEAN, `old_lp24_probe`, counted `OLD_LP24` at
//! 22ea0dc0 (plan § Measured) and went with `SvfFilter`.

use super::Clean;
use crate::hw::{BLOCK_SIZE, SAMPLE_RATE};
use crate::params::FilterParams;

/// CLEAN in mode `M` (a `FilterMode` discriminant), `process`'s prelude
/// with the loop forced: the steady one, or the moving one if `MOVING`.
#[inline(never)]
pub fn clean_probe<const M: u8, const MOVING: bool>(
    f: &mut Clean,
    buf: &mut [f32; BLOCK_SIZE],
    p: &FilterParams,
) {
    let r = f.ramps(p, SAMPLE_RATE);
    if MOVING {
        f.moving::<M>(buf, r);
    } else {
        f.steady::<M>(buf, r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filter::FilterMode;

    /// Each probe is LP24's `process` on its own kind of block: steady
    /// blocks, then a cutoff and RES move, RES alone (a moving block too),
    /// and past 0.497·fs.
    #[test]
    fn clean_probe_is_process_at_lp24() {
        let mut p = FilterParams::default();
        (p.cutoff, p.resonance, p.drive) = (800.0, 0.7, 0.3);
        assert!(p.set_mode(FilterMode::Lp24));
        let (mut a, mut b) = (Clean::new(), Clean::new());
        let mut x = 0.37f32;
        for block in 0..6 {
            let moving = matches!(block, 2..=4);
            match block {
                2 => (p.cutoff, p.resonance) = (3000.0, 0.4),
                3 => p.resonance = 0.5,
                4 => p.cutoff = SAMPLE_RATE as f32 * 0.6,
                _ => {}
            }
            let mut want = [0.0f32; BLOCK_SIZE];
            for s in want.iter_mut() {
                x = crate::dsp::clampf(x * 3.9 * (1.0 - x), 0.01, 0.99);
                *s = x - 0.5;
            }
            let mut got = want;
            a.process(&mut want, &p, SAMPLE_RATE);
            if moving {
                clean_probe::<2, true>(&mut b, &mut got, &p);
            } else {
                clean_probe::<2, false>(&mut b, &mut got, &p);
            }
            assert_eq!(got, want, "block {block}");
        }
    }
}
