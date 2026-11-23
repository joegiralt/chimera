// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The meters' block (ADR 0293), bank 2's: an S4 group (`bank2.x`,
//! `*10part_peaks4kern*`), reached through `PeakDoor`.

use crate::hw::{BLOCK_SIZE, MAX_PARTS};
use crate::instrument::Sides;

use super::{PART_PEAKS, Peak};

/// A block's Part buses as the meters read them: each bus (the mid), its
/// side where one was written, and which were written.
pub struct PeaksBlock<'a> {
    pub buses: &'a [[f32; BLOCK_SIZE]; MAX_PARTS],
    pub sides: &'a Sides,
    pub written: &'a [bool; MAX_PARTS],
}

/// Each written Part's peak this block into `PART_PEAKS`: `|mid|`, or
/// `|mid| + |side|` (the louder of L and R) where a side was written. A
/// Part with nothing written raises nothing. Sized: it runs only while
/// the page is up, and unrolled it was 1.5 KB.
#[optimize(size)]
pub(super) fn run(b: PeaksBlock<'_>) {
    let PeaksBlock {
        buses,
        sides,
        written,
    } = b;
    for (p, bus) in buses.iter().enumerate() {
        if !written[p] {
            continue;
        }
        let peak = match &sides[p] {
            Some(side) => bus
                .iter()
                .zip(side)
                .fold(0.0f32, |m, (b, s)| m.max(libm::fabsf(*b) + libm::fabsf(*s))),
            None => bus.iter().fold(0.0f32, |m, b| m.max(libm::fabsf(*b))),
        };
        PART_PEAKS[p].raise(Peak::new(peak));
    }
}
