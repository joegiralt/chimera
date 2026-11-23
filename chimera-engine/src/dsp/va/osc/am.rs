// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! AM's OSC 1 (#660, ADR 0321): its plain render with its steps listed,
//! not corrected, for AM's product to correct (`combine::AmKinks`). Its
//! own module, so with `bank2` it links into an S4 group with AM's render
//! (`bank2.x`, `*2va3osc2am*`): S0-S3, where `door`'s instances go, is
//! under ADR 0251's 8 KB.

use chimera_hal::BLOCK_SIZE;

use super::{Blk, Kind, PM_JUMPS, Ramps, VarOsc, ZEROS};
use crate::dsp::va::stability::Stability;
use crate::dsp::va::stability::{JitterScale, WrapJitter};

/// OSC 1's listing render (`<64, true, false, true, false>` with `PH`):
/// the plain instance's wraps and sub flips in `mk`, its steps left naive
/// and listed (`VarOsc::listed_steps`, up to `PM_JUMPS` a block; past them
/// corrected as ever), never marked.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn osc1_steps(
    o: &mut VarOsc,
    out: &mut [f32; BLOCK_SIZE],
    mk: &mut [[f32; 2]; BLOCK_SIZE],
    r: &Ramps,
    jit: &mut WrapJitter,
    stab: &mut Stability,
    js: JitterScale,
) {
    let mut draw = || js.draw(stab);
    o.pm_jumps = PM_JUMPS;
    o.render_in::<BLOCK_SIZE, true, false, true, false, true>(
        out,
        mk,
        r,
        jit,
        &mut draw,
        &ZEROS,
        (0.0, 0.0),
    );
}

/// `VarOsc::run`'s listing instance with the sub's flips (`PH`, `FLIP`, no
/// `MOD`): AM's OSC 1 only.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn run<
    A: Kind,
    B: Kind,
    const N: usize,
    const REC: bool,
    const SYNC: bool,
    const FLIP: bool,
    const MOD: bool,
    const PH: bool,
>(
    o: &mut VarOsc,
    blk: &Blk,
    span: (usize, usize),
    r: &Ramps,
    out: &mut [f32; N],
    mk: &mut [[f32; 2]; N],
    jit: &mut WrapJitter,
    draw: &mut impl FnMut() -> WrapJitter,
    m: &[f32; N],
    k: (f32, f32),
) {
    o.run_in::<A, B, N, REC, SYNC, FLIP, MOD, PH, false>(blk, span, r, out, mk, jit, draw, m, k);
}
