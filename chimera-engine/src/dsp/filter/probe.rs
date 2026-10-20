//! Static-cost probes (spec § Cost): each runs one block of one path,
//! `#[inline(never)]`, so `tools/loop_count.py` finds its loops by name in the
//! `filter-probes` ELF, whose `FILTER_PROBES` keeps them linked.
//!
//! Today's LP24 before CLEAN, `old_lp24_probe`, counted `OLD_LP24` at
//! 22ea0dc0 (plan § Measured) and went with `SvfFilter`.

use core::hint::black_box;

use super::clean::clean_k;
use super::k2040::VoiceSpec;
use super::{Clean, Dirty, FilterMods, K2040, Ladder, TapShape, tap_g_range};
use crate::dsp::voice_fx::{Ends, fold};
use crate::hw::{BLOCK_SIZE, SAMPLE_RATE};
use crate::params::FilterParams;
use crate::taps::TapLane;

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
        f.moving::<M>(buf, r, None);
    } else {
        f.steady::<M>(buf, r);
    }
}

/// The 2040's two loops, `process`'s ramps with the loop forced: the steady
/// one, or the moving one if `MOVING`; VOICE `V` (`p.voice` must agree).
#[inline(never)]
pub fn k2040_probe<V: VoiceSpec, const MOVING: bool>(
    f: &mut K2040,
    buf: &mut [f32; BLOCK_SIZE],
    p: &FilterParams,
) {
    let r = f.ramps(p, SAMPLE_RATE, &FilterMods::NONE);
    let (_, sum) = if MOVING {
        f.moving::<V>(buf, r, None, (super::Fed::OFF, (0.0, 1.0)), |_, _| {})
    } else {
        f.steady::<V>(buf, r, |_, _| {})
    };
    f.settle(sum);
}

/// DIRTY in mode `M` (`DIRTY_MODES`' index), `process` with the mode
/// forced: one loop serves steady and moving blocks (`p.mode()` must agree).
#[inline(never)]
pub fn dirty_probe<const M: u8>(f: &mut Dirty, buf: &mut [f32; BLOCK_SIZE], p: &FilterParams) {
    let r = f.prelude(buf, p, SAMPLE_RATE, &FilterMods::NONE);
    let sum = f.run::<M>(buf, r, None, super::Fed::OFF, SAMPLE_RATE);
    f.settle(sum);
}

/// LADDER in mode `M` (`LADDER_MODES`' index), `process`'s ramps with the
/// loop forced: the steady one, or the moving one if `MOVING` (`p.mode()`
/// must agree).
#[inline(never)]
pub fn ladder_probe<const M: usize, const MOVING: bool>(
    f: &mut Ladder,
    buf: &mut [f32; BLOCK_SIZE],
    p: &FilterParams,
) {
    let r = f.ramps(p, SAMPLE_RATE, &FilterMods::NONE);
    if super::ladder::hold_huge(buf).is_none() {
        f.silence(buf);
    } else if MOVING {
        f.moving(buf, r, None, M);
    } else {
        f.steady(buf, r, M);
    }
}

/// A FREQ and a Q lane on `buf`'s own samples, opaque to the optimiser.
fn lanes(rows: &[[f32; BLOCK_SIZE]; 2]) -> FilterMods<'_> {
    let l = black_box(TapLane {
        rows,
        from: [0.5, 0.25],
        to: [0.5, 0.25],
    });
    FilterMods {
        freq: Some(l),
        q: Some(l),
        ..FilterMods::NONE
    }
}

/// The taps' pass over a moving block's `g` and `k` (`TapShape::apply`),
/// FREQ and Q both: one loop for every type, ahead of its moving loop.
#[inline(never)]
pub fn tap_shape_probe(buf: &mut [f32; BLOCK_SIZE]) {
    let rows = black_box([*buf; 2]);
    if let Some(t) = TapShape::new(&lanes(&rows), tap_g_range(SAMPLE_RATE), clean_k) {
        let (mut g, mut k) = (*buf, *buf);
        t.apply(&mut g, &mut k);
        for ((s, g), k) in buf.iter_mut().zip(g).zip(k) {
            *s = g + k;
        }
    }
}

/// The tap-VCA's loop, both ops present.
#[inline(never)]
pub fn tap_vca_probe(buf: &mut [f32; BLOCK_SIZE]) {
    let rows = black_box([*buf; 2]);
    if let Some(l) = lanes(&rows).freq {
        crate::taps::tap_vca(buf, &l);
    }
}

/// FOLD's tapped loop.
#[inline(never)]
pub fn fold_tap_probe(buf: &mut [f32; BLOCK_SIZE], e: &Ends) {
    let rows = black_box([*buf; 2]);
    let l = lanes(&rows).freq;
    fold::run(buf, e, false, l.as_ref());
}

/// CLEAN's glide loop (`Clean::stepped`) in mode `M`: `STEPS` sub-blocks.
#[inline(never)]
pub fn clean_stepped_probe<const M: u8>(
    f: &mut Clean,
    buf: &mut [f32; BLOCK_SIZE],
    p: &FilterParams,
) {
    let r = f.prelude(buf, p, SAMPLE_RATE, &FilterMods::NONE);
    f.stepped::<M>(buf, r);
}

/// The 2040's glide loop (`K2040::stepped`) on voice `V`.
#[inline(never)]
pub fn k2040_stepped_probe<V: VoiceSpec>(
    f: &mut K2040,
    buf: &mut [f32; BLOCK_SIZE],
    p: &FilterParams,
) {
    let r = f.ramps(p, SAMPLE_RATE, &FilterMods::NONE);
    let (_, sum) = f.stepped::<V>(buf, r, |_, _| {});
    f.settle(sum);
}

/// LADDER's glide loop (`Ladder::stepped`) in mode `M`.
#[inline(never)]
pub fn ladder_stepped_probe<const M: usize>(
    f: &mut Ladder,
    buf: &mut [f32; BLOCK_SIZE],
    p: &FilterParams,
) {
    let r = f.ramps(p, SAMPLE_RATE, &FilterMods::NONE);
    if super::ladder::hold_huge(buf).is_none() {
        f.silence(buf);
    } else {
        f.stepped(buf, r, M);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::filter::{FilterKind, FilterMode};

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

    /// Each probe is `process` on its own kind of block, in every mode:
    /// steady blocks, then a cutoff and RES move, RES alone (a moving block
    /// too), and past 0.497·fs.
    #[test]
    fn clean_probe_is_process() {
        probe_is_process::<0>(FilterMode::Lp6);
        probe_is_process::<1>(FilterMode::Lp12);
        probe_is_process::<2>(FilterMode::Lp24);
        probe_is_process::<3>(FilterMode::Bp12);
        probe_is_process::<4>(FilterMode::Bp24);
        probe_is_process::<5>(FilterMode::Hp24);
        probe_is_process::<6>(FilterMode::Notch);
        probe_is_process::<7>(FilterMode::Phaser);
    }

    /// LADDER's probe is `process` on each kind of block, every mode.
    #[test]
    fn ladder_probe_is_process() {
        ladder_is_process::<0>();
        ladder_is_process::<1>();
        ladder_is_process::<2>();
        ladder_is_process::<3>();
        ladder_is_process::<4>();
    }

    fn ladder_is_process<const M: usize>() {
        let mode = crate::dsp::filter::ladder::LADDER_MODES[M];
        let mut p = FilterParams::default();
        p.set_kind(FilterKind::Ladder);
        assert!(p.set_mode(mode));
        (p.cutoff, p.resonance, p.level) = (800.0, 0.7, 0.75);
        let (mut a, mut b) = (Ladder::new(), Ladder::new());
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
                ladder_probe::<M, true>(&mut b, &mut got, &p);
            } else {
                ladder_probe::<M, false>(&mut b, &mut got, &p);
            }
            assert_eq!(got, want, "{mode:?} block {block}");
        }
    }

    /// The 2040's probe is `process` on each kind of block, both voices.
    #[test]
    fn k2040_probe_is_process() {
        k2040_is_process::<crate::dsp::filter::k2040::voice::Rev2>();
        k2040_is_process::<crate::dsp::filter::k2040::voice::Exp>();
    }

    fn k2040_is_process<V: VoiceSpec>() {
        let v = V::VOICE;
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
                k2040_probe::<V, true>(&mut b, &mut got, &p);
            } else {
                k2040_probe::<V, false>(&mut b, &mut got, &p);
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
