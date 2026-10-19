//! The VA's `MOD` oscillators behind bank 2's door (ADR 0163). Every
//! `MOD` render the engine plays is an entry here, its runs `run`'s
//! instances, so with `bank2` the module links into `.bank2` whole
//! (`bank2.x`, `*3osc4door*`) and bank 1 reaches it through `VaDoor`.
//! Without `bank2` it is bank 1's, called directly. `block`, `enter`
//! and the shared helpers stay in bank 1; bank 2 calls them.
//!
//! A new instance is one function here and one `VaDoor` field: the glob
//! already takes it.

use chimera_hal::BLOCK_SIZE;

use super::super::blep::blep_into;
use super::{Blk, Kind, Leg, Lin, PM_JUMPS, Ramps, VarOsc, ZEROS, frac};
use crate::dsp::va::stability::Stability;

/// A `MOD` render as the door carries it: the oscillator, its block, its
/// marks, its lanes and jitter, STABILITY (the jitter's draws, `1 + jd ·
/// word`), the modulation `m` and its form `k` (ADR 0158 point 2).
pub type ModRender = fn(
    &mut VarOsc,
    &mut [f32; BLOCK_SIZE],
    &mut [[f32; 2]; BLOCK_SIZE],
    &Ramps,
    &mut f32,
    &mut Stability,
    f32,
    &[f32; BLOCK_SIZE],
    (f32, f32),
);

/// The VA's entries in the door's table.
#[cfg(feature = "bank2")]
#[repr(C)]
pub struct VaDoor {
    /// OSC 1's recording, flipping `MOD` instance: FM `k = (1, 0)`.
    pub osc1_mod: ModRender,
    /// PHASE's OSC 1, `k = (0, 1)`, taking OSC 2's steps as jumps.
    pub osc1_phase: ModRender,
    /// PHASE's OSC 2, plain, reporting its steps (`m`, `k` unread).
    pub osc2_steps: ModRender,
}

#[cfg(feature = "bank2")]
pub const DOOR: VaDoor = VaDoor {
    osc1_mod,
    osc1_phase,
    osc2_steps,
};

/// OSC 1's `MOD` render (`<64, true, false, true, true>`): it records the
/// wraps and the sub's flips on its clock.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn osc1_mod(
    o: &mut VarOsc,
    out: &mut [f32; BLOCK_SIZE],
    mk: &mut [[f32; 2]; BLOCK_SIZE],
    r: &Ramps,
    jit: &mut f32,
    stab: &mut Stability,
    jd: f32,
    m: &[f32; BLOCK_SIZE],
    k: (f32, f32),
) {
    let mut draw = || 1.0 + jd * stab.jitter_word() as f32;
    o.render_in::<BLOCK_SIZE, true, false, true, true, false>(out, mk, r, jit, &mut draw, m, k);
}

/// PHASE's OSC 1 (`<64, true, false, true, true>` with `PH`, ADR 0160):
/// `osc1_mod`'s, and a mark of OSC 2's steps (`osc2_steps`) in `mk` is a
/// jump of the read (`jump`); `m` is the read's rate without them.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn osc1_phase(
    o: &mut VarOsc,
    out: &mut [f32; BLOCK_SIZE],
    mk: &mut [[f32; 2]; BLOCK_SIZE],
    r: &Ramps,
    jit: &mut f32,
    stab: &mut Stability,
    jd: f32,
    m: &[f32; BLOCK_SIZE],
    k: (f32, f32),
) {
    let mut draw = || 1.0 + jd * stab.jitter_word() as f32;
    o.render_in::<BLOCK_SIZE, true, false, true, true, true>(out, mk, r, jit, &mut draw, m, k);
}

/// PHASE's OSC 2: plain, its steps marked, not corrected, up to
/// `PM_JUMPS` a block (ADR 0160). `out` is naive at them; `mk[i + 1]` is
/// `[JUMP − 1 − t, Σ s]` for the steps `s` crossed before sample `i`, `t`
/// their step-weighted time ago, where `out[i + 1]` takes them (one late);
/// the last sample's land in `mk[0]`. Other marks are left as they were
/// (never under `JUMP`). Corners, and steps past the budget, are corrected
/// as ever. `m` and `k` are unread.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn osc2_steps(
    o: &mut VarOsc,
    out: &mut [f32; BLOCK_SIZE],
    mk: &mut [[f32; 2]; BLOCK_SIZE],
    r: &Ramps,
    jit: &mut f32,
    stab: &mut Stability,
    jd: f32,
    _: &[f32; BLOCK_SIZE],
    _: (f32, f32),
) {
    let mut draw = || 1.0 + jd * stab.jitter_word() as f32;
    o.pm_jumps = PM_JUMPS;
    o.render_in::<BLOCK_SIZE, false, false, false, false, true>(
        out,
        mk,
        r,
        jit,
        &mut draw,
        &ZEROS,
        (0.0, 0.0),
    );
}

/// A jump of the read by `jj` cycles `t` samples ago, `rr` the read's rate
/// around it (PHASE's modulator step, ADR 0160): the read up to it, its
/// crossings as any sample's; the jump, one step of the output at `t`
/// (the value after it less the value before), its edges re-anchored and
/// none corrected on its own. The read is then set back by the motion up
/// to the jump: the sample's own motion (a whole `rr`) and crossings play
/// the rest, after it. Out of line: the loop's way with no jump stays as
/// it was.
#[cold]
#[inline(never)]
pub(super) fn jump<A: Kind, B: Kind>(
    o: &mut VarOsc,
    s: &mut Leg,
    (t, jj, rr): (f32, f32, f32),
    od: &[f32; 4],
    (ca, cb): ([Lin; 3], [Lin; 3]),
) {
    let edged = |k: usize| if k < 2 { A::EDGES } else { B::EDGES };
    let bias = |l: Lin| if A::BIAS && B::BIAS { -0.0 } else { l.v };
    let a = 1.0 - t;
    let mut rep = [0.0f32; 2];
    for k in 0..4 {
        if edged(k) {
            s.u[k] += (rr - od[k]) * a;
        }
    }
    let (u, h) = (&mut s.u, &mut s.h);
    let ha = (&mut s.held, &mut s.acc);
    if A::EDGES {
        for e in 0..2 {
            let v = rr - od[e];
            let c = (&mut *ha.0, &mut *ha.1);
            o.cross::<A, true, false>(e, &mut u[e], t, (0.0, 0.0), v, ca, h, c, &mut rep);
        }
    }
    if B::EDGES {
        for e in 0..2 {
            let v = rr - od[e + 2];
            let c = (&mut *ha.0, &mut *ha.1);
            o.cross::<B, true, false>(e, &mut u[e + 2], t, (0.0, 0.0), v, cb, h, c, &mut rep);
        }
    }
    let rd = frac(s.rd + rr * a);
    let (va, vb) = (ca.map(|l| l.v), [bias(cb[0]), cb[1].v, cb[2].v]);
    let y0 = A::value(va, rd, [u[0], u[1]], *h) + B::value(vb, rd, [u[2], u[3]], *h);
    let rd = frac(rd + jj);
    for (k, u) in u.iter_mut().enumerate() {
        if edged(k) {
            *u = frac(*u + jj);
        }
    }
    // the pulse: high where the phase since the rise is the shorter
    // (`high`)
    if A::FLIPS {
        *h = if u[0] < u[1] { 1.0 } else { 0.0 };
    }
    if B::FLIPS {
        *h = if u[2] < u[3] { 1.0 } else { 0.0 };
    }
    let y1 = A::value(va, rd, [u[0], u[1]], *h) + B::value(vb, rd, [u[2], u[3]], *h);
    if o.apply(t, y1 - y0, 0.0) {
        blep_into(ha.0, ha.1, t, y1 - y0);
    }
    for k in 0..4 {
        if edged(k) {
            u[k] -= (rr - od[k]) * a;
        }
    }
    s.rd = rd - rr * a;
}

/// `VarOsc::run`'s `MOD` instances, and PHASE's (`PH`).
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
    jit: &mut f32,
    draw: &mut impl FnMut() -> f32,
    m: &[f32; N],
    k: (f32, f32),
) {
    o.run_in::<A, B, N, REC, SYNC, FLIP, MOD, PH>(blk, span, r, out, mk, jit, draw, m, k);
}
