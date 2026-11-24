// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! TRON's run-out onset (#1126), out of line in an S4 group
//! (`chimera-stm32`'s `build.rs`): it runs a block a note, and S0-S3,
//! where the rest of TAPE lies, is full.

/// The block the tape runs out in: `out` under a gain that eases, by a
/// smoothstep across the block, from the fall it was on (`was` a sample, 1
/// held) into the run-out's one-pole (`k` a sample), both from `g`. A
/// one-pole starting at full slope bent the wave there, a click on a loud
/// note. The one-pole's own gain is stepped as the release steps it, and
/// returned: the blocks after fall as they always did, bit for bit.
#[inline(never)]
pub fn onset(out: &mut [f32], mut g: f32, (was, k): (f32, f32)) -> f32 {
    let (mut h, by) = (g, 1.0 / out.len() as f32);
    for (i, y) in out.iter_mut().enumerate() {
        let x = i as f32 * by;
        let w = 1.0 - x * x * (3.0 - 2.0 * x);
        *y *= g + (h - g) * w;
        g *= k;
        h *= was;
    }
    g
}
