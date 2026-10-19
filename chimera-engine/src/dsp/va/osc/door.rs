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

use super::{Blk, Kind, Ramps, VarOsc};
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
    /// OSC 1's recording, flipping `MOD` instance: FM `k = (1, 0)`,
    /// PHASE `(0, 1)`.
    pub osc1_mod: ModRender,
    /// LOOPFM's whole render (`VaEngine::render_loopfm`, with
    /// `lbfm::render_loop`).
    pub loopfm: crate::dsp::va::LoopFmRender,
}

#[cfg(feature = "bank2")]
pub const DOOR: VaDoor = VaDoor {
    osc1_mod,
    loopfm: crate::dsp::va::VaEngine::render_loopfm,
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
    o.render_in::<BLOCK_SIZE, true, false, true, true>(out, mk, r, jit, &mut draw, m, k);
}

/// `VarOsc::run`'s `MOD` instances.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub(super) fn run<
    A: Kind,
    B: Kind,
    const N: usize,
    const REC: bool,
    const SYNC: bool,
    const FLIP: bool,
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
    o.run_in::<A, B, N, REC, SYNC, FLIP, true>(blk, span, r, out, mk, jit, draw, m, k);
}
