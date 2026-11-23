// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The stereo's blocks (ADR 0294), bank 2's: an S4 group (`bank2.x`,
//! `*6stereo4kern*`), reached through `StereoDoor`.

use crate::dsp::drum::types::SampleRate;
use crate::dsp::ease::{at, ease_coeff, step_of};
use crate::hw::{BLOCK_SIZE, MAX_PARTS};
use crate::instrument::{PartAudio, Sides};

use super::{PartWidths, VoicePan, Width};

/// A note's block `x` placed from `from` to `to`, ramped across it: its
/// mid left in `x`, its side added to `side` (written if empty), as a
/// pad's `MidSide::split`. The caller skips a note centred both ends.
pub(super) fn pan(
    x: &mut [f32; BLOCK_SIZE],
    side: &mut Option<[f32; BLOCK_SIZE]>,
    from: VoicePan,
    to: VoicePan,
) {
    let (a, b) = (from.gains(), to.gains());
    let s = side.get_or_insert([0.0; BLOCK_SIZE]);
    if a == b {
        for (x, s) in x.iter_mut().zip(s) {
            *s += *x * a.side;
            *x *= a.mid;
        }
        return;
    }
    let dm = step_of((a.mid, b.mid), BLOCK_SIZE);
    let ds = step_of((a.side, b.side), BLOCK_SIZE);
    for (i, (x, s)) in x.iter_mut().zip(s).enumerate() {
        *s += *x * at(a.side, ds, i);
        *x *= at(a.mid, dm, i);
    }
}

/// Closer than this to its WIDTH, a Part's lands on it (`Ease`'s −100 dB).
const LAND: f32 = 1e-5;

/// Each Part's side times its WIDTH, eased from the last block's toward
/// the stored one (20 ms, `ease_coeff`) and ramped across the block. A
/// Part with no side lands on its WIDTH unheard; one at unity both ends
/// is left as it is, bit for bit; with every Part so, it returns at once.
pub(super) fn widths(
    w: &mut PartWidths,
    sides: &mut Sides,
    parts: &[PartAudio; MAX_PARTS],
    sample_rate: SampleRate,
) {
    // Every WIDTH unity, last block and this: nothing to ease or scale.
    if w.at_rest(|p| parts[p.index()].params.out.width) {
        return;
    }
    let k = ease_coeff(sample_rate.get());
    for (p, (side, part)) in sides.iter_mut().zip(parts).enumerate() {
        let to = part.params.out.width;
        let Some(side) = side else {
            w.0[p] = to;
            continue;
        };
        let from = w.0[p];
        let mut next = to.get() + k * (from.get() - to.get());
        if (next - to.get()).abs() < LAND {
            next = to.get();
        }
        let next = Width::new(next);
        w.0[p] = next;
        if from == next {
            if next != Width::UNITY {
                side.iter_mut().for_each(|s| *s *= next.get());
            }
            continue;
        }
        let d = step_of((from.get(), next.get()), BLOCK_SIZE);
        for (i, s) in side.iter_mut().enumerate() {
            *s *= at(from.get(), d, i);
        }
    }
}
