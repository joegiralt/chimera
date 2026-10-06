//! Static-cost probes (spec § Cost): each runs one block of one path,
//! `#[inline(never)]`, so `tools/loop_count.py` finds its loops by name in the
//! bench ELF. The bench's `FILTER_PROBES` keeps them linked.
//!
//! Today's LP24 before CLEAN, `old_lp24_probe`, counted `OLD_LP24` at
//! 22ea0dc0 (plan § Measured) and went with `SvfFilter`.

use super::{Clean, Dirty, FilterMods, K2040};
use crate::hw::{BLOCK_SIZE, SAMPLE_RATE};
use crate::params::FilterParams;

/// CLEAN in mode `M` (a `FilterMode` discriminant), `process`'s prelude
/// and LEVEL's stage with the loop forced: the steady one, or the moving one
/// if `MOVING`. LEVEL's three loops count apart from the filter's: the gain
/// alone (gate shut, LEVEL off ½), the clipper faded in, and the clipper
/// fading, each with its peak.
#[inline(never)]
pub fn clean_probe<const M: u8, const MOVING: bool>(
    f: &mut Clean,
    buf: &mut [f32; BLOCK_SIZE],
    p: &FilterParams,
) {
    let r = f.prelude(buf, p, SAMPLE_RATE, &FilterMods::NONE);
    if MOVING {
        f.moving::<M>(buf, r);
    } else {
        f.steady::<M>(buf, r);
    }
}

/// The 2040's two loops, `process`'s ramps with the loop forced: the steady
/// one, or the moving one if `MOVING`; REV2's output, or EXP's 3080 if
/// `EXP` (`p.voice` must agree).
#[inline(never)]
pub fn k2040_probe<const EXP: bool, const MOVING: bool>(
    f: &mut K2040,
    buf: &mut [f32; BLOCK_SIZE],
    p: &FilterParams,
) {
    let r = f.ramps(p, SAMPLE_RATE, &FilterMods::NONE);
    let (_, sum) = if MOVING {
        f.moving::<EXP>(buf, r, |_, _| {})
    } else {
        f.steady::<EXP>(buf, r, |_, _| {})
    };
    f.settle(sum);
}

/// DIRTY in mode `M` (`DIRTY_MODES`' index), `process` with the mode
/// forced: one loop serves steady and moving blocks (`p.mode()` must agree).
#[inline(never)]
pub fn dirty_probe<const M: u8>(f: &mut Dirty, buf: &mut [f32; BLOCK_SIZE], p: &FilterParams) {
    let r = f.prelude(buf, p, SAMPLE_RATE, &FilterMods::NONE);
    let sum = f.run::<M>(buf, r);
    f.settle(sum);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filter::{FilterKind, FilterMode, K2040Voice};

    /// DIRTY's probe is `process` in every mode, on steady blocks, a FREQ
    /// and RES move, RES alone, past the ceiling, and LEVEL over unity.
    #[test]
    fn dirty_probe_is_process() {
        dirty_is_process::<0>(FilterMode::Lp12);
        dirty_is_process::<1>(FilterMode::Bp12);
        dirty_is_process::<2>(FilterMode::Hp12);
        dirty_is_process::<3>(FilterMode::Notch);
    }

    fn dirty_is_process<const M: u8>(mode: FilterMode) {
        let mut p = FilterParams::default();
        p.set_kind(FilterKind::Dirty);
        assert!(p.set_mode(mode));
        (p.cutoff, p.resonance, p.level) = (800.0, 0.7, 0.75);
        let (mut a, mut b) = (Dirty::new(), Dirty::new());
        let mut x = 0.37f32;
        for block in 0..6 {
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
            a.process(&mut want, &p, SAMPLE_RATE, &FilterMods::NONE);
            dirty_probe::<M>(&mut b, &mut got, &p);
            assert_eq!(got, want, "{mode:?} block {block}");
        }
    }

    /// Each probe is `process` on its own kind of block, at LP4 and at
    /// NOTCH2: steady blocks, then a cutoff and RES move, RES alone (a
    /// moving block too), and past 0.497·fs.
    #[test]
    fn clean_probe_is_process() {
        probe_is_process::<2>(FilterMode::Lp24);
        probe_is_process::<7>(FilterMode::Phaser);
    }

    /// The 2040's probe is `process` on each kind of block, both voices.
    #[test]
    fn k2040_probe_is_process() {
        k2040_is_process::<false>(K2040Voice::Rev2);
        k2040_is_process::<true>(K2040Voice::Exp);
    }

    fn k2040_is_process<const EXP: bool>(v: K2040Voice) {
        let mut p = FilterParams::default();
        p.set_kind(FilterKind::K2040);
        (p.cutoff, p.resonance, p.level, p.voice) = (800.0, 0.7, 0.75, v);
        let (mut a, mut b) = (K2040::new(), K2040::new());
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
            a.process(&mut want, &p, SAMPLE_RATE, &FilterMods::NONE);
            if moving {
                k2040_probe::<EXP, true>(&mut b, &mut got, &p);
            } else {
                k2040_probe::<EXP, false>(&mut b, &mut got, &p);
            }
            assert_eq!(got, want, "{v:?} block {block}");
        }
    }

    fn probe_is_process<const M: u8>(mode: FilterMode) {
        let mut p = FilterParams::default();
        (p.cutoff, p.resonance, p.level) = (800.0, 0.7, 0.75);
        assert!(p.set_mode(mode));
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
            a.process(&mut want, &p, SAMPLE_RATE, &FilterMods::NONE);
            if moving {
                clean_probe::<M, true>(&mut b, &mut got, &p);
            } else {
                clean_probe::<M, false>(&mut b, &mut got, &p);
            }
            assert_eq!(got, want, "{mode:?} block {block}");
        }
    }
}
