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
use super::{Blk, Kind, Leg, Lin, PM_JUMPS, PulseQuiet, Ramps, Saw, Solo, VarOsc, ZEROS, frac};
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

/// A `MOD` render as `VaDoor` holds it: called, never handed out.
#[repr(transparent)]
pub struct ModEntry(ModRender);

impl ModEntry {
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
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
        (self.0)(o, out, mk, r, jit, stab, jd, m, k)
    }
}

pub const OSC1_MOD: ModEntry = ModEntry(osc1_mod);
pub const OSC2_REV: ModEntry = ModEntry(osc2_rev);
pub const OSC1_PHASE: ModEntry = ModEntry(osc1_phase);
pub const OSC2_STEPS: ModEntry = ModEntry(osc2_steps);

/// A plain render with WAVE on an anchor (`VarOsc::render_solo`, ADR
/// 0251): the oscillator, its instance and solo, its block, its marks, its
/// lanes and jitter, STABILITY (the jitter's draws, `1 + jd · word`).
pub type SoloRender = fn(
    &mut VarOsc,
    SoloRun,
    Solo,
    &mut [f32; BLOCK_SIZE],
    &mut [[f32; 2]; BLOCK_SIZE],
    &Ramps,
    &mut f32,
    &mut Stability,
    f32,
) -> bool;

/// A solo render as `VaDoor` holds it: called, never handed out.
#[repr(transparent)]
pub struct SoloEntry(SoloRender);

impl SoloEntry {
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        o: &mut VarOsc,
        run: SoloRun,
        solo: Solo,
        out: &mut [f32; BLOCK_SIZE],
        mk: &mut [[f32; 2]; BLOCK_SIZE],
        r: &Ramps,
        jit: &mut f32,
        stab: &mut Stability,
        jd: f32,
    ) -> bool {
        (self.0)(o, run, solo, out, mk, r, jit, stab, jd)
    }
}

/// Which plain instance a solo plays: the engine's three (`VaEngine::run`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoloRun {
    /// OSC 1, marking its wraps and the sub's flips.
    Osc1,
    /// OSC 2 as SYNC's slave, resetting at OSC 1's wraps.
    Osc2Sync,
    /// OSC 2, plain.
    Osc2,
}

/// The solos' table, at its own slot after the door (`VA_SOLO_DOOR_ADDR`,
/// `bank2.x`): the door's table has no room. Its entry is private, called
/// only through `through`.
#[repr(C)]
pub struct SoloDoor {
    magic: Magic,
    solo: SoloEntry,
}

/// `SoloDoor`'s mark: a table at the slot that isn't this one fails the
/// boot's door check (`door_ok`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Magic(u32);

const MAGIC: Magic = Magic(0xB2A5_0251);

impl SoloDoor {
    const OWN: SoloDoor = SoloDoor {
        magic: MAGIC,
        solo: SoloEntry(solo),
    };

    /// The plain render's solo (`solo`).
    #[inline(always)]
    pub fn solo(&self) -> &SoloEntry {
        &self.solo
    }
}

#[cfg(feature = "bank2")]
// SAFETY: `bank2.x` links `SOLO_DOOR`, a `SoloDoor`, at `VA_SOLO_DOOR_ADDR`
// in every firmware build with `bank2` (its assert, `__bank2_va_solo_door`).
const SLOT: crate::bank2::DoorSlot<SoloDoor> =
    unsafe { crate::bank2::DoorSlot::new(crate::bank2::VA_SOLO_DOOR_ADDR) };

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.door.va_solo")]
#[used]
static SOLO_DOOR: SoloDoor = SoloDoor::OWN;

/// The solos' table in judged door `j` answers (the boot's door check, run
/// before the door opens).
#[cfg(feature = "bank2")]
pub fn door_ok(j: &crate::bank2::Judged) -> bool {
    j.with_slot(SLOT, |d| d.magic == MAGIC)
}

/// `f` on the solos' table: bank 2's through the door on the unit, `None`
/// closed; this build's own elsewhere. A host test under `bank2` answers
/// as the unit does: only while the door cell is open.
#[inline(always)]
pub fn through<R>(f: impl FnOnce(&SoloDoor) -> R) -> Option<R> {
    #[cfg(all(feature = "bank2", target_os = "none"))]
    {
        crate::bank2::DOOR_CELL.with_slot(SLOT, f)
    }
    #[cfg(not(all(feature = "bank2", target_os = "none")))]
    {
        #[cfg(feature = "bank2")]
        if !crate::bank2::DOOR_CELL.is_open() {
            return None;
        }
        Some(f(&SoloDoor::OWN))
    }
}

/// The VA's entries in the door's table.
#[cfg(feature = "bank2")]
#[repr(C)]
pub struct VaDoor {
    /// OSC 1's recording, flipping `MOD` instance: FM `k = (1, 0)`.
    pub osc1_mod: ModEntry,
    /// LOOPFM's whole render (`VaEngine::render_loopfm`, with
    /// `lbfm::render_loop`).
    loopfm: crate::dsp::va::LoopFmRender,
    /// OSC 2's reversing instance: SOFT SYNC, `k = (0, 0)` (ADR 0161).
    pub osc2_rev: ModEntry,
    /// PHASE's OSC 1, `k = (0, 1)`, taking OSC 2's steps as jumps.
    pub osc1_phase: ModEntry,
    /// PHASE's OSC 2, plain, reporting its steps (`m`, `k` unread).
    pub osc2_steps: ModEntry,
}

#[cfg(feature = "bank2")]
pub const DOOR: VaDoor = VaDoor {
    osc1_mod: OSC1_MOD,
    loopfm: crate::dsp::va::VaEngine::render_loopfm,
    osc2_rev: OSC2_REV,
    osc1_phase: OSC1_PHASE,
    osc2_steps: OSC2_STEPS,
};

#[cfg(feature = "bank2")]
impl VaDoor {
    #[inline(always)]
    pub(in crate::dsp::va) fn loopfm(
        &self,
        e: &mut crate::dsp::va::VaEngine,
        out: &mut [f32; BLOCK_SIZE],
        l: &crate::dsp::va::Lanes,
        gain: crate::dsp::va::Gain,
        x: f32,
        wave: [f32; 2],
    ) {
        (self.loopfm)(e, out, l, gain, x, wave)
    }
}

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

/// OSC 2's reversing render (`<64, false, true, false, true>`): a mark is
/// a turn, its read reverses there (SOFT SYNC, ADR 0161).
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn osc2_rev(
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
    o.render_in::<BLOCK_SIZE, false, true, false, true, false>(out, mk, r, jit, &mut draw, m, k);
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

/// A plain render's solo, if bank 2 holds its instance: OSC 1's shared
/// one (`<64, true, false, true>`), OSC 2's as SYNC's slave (`<64, false,
/// true, false>`) or plain (`<64, false, false, false>`). `false`, nothing
/// played: the pair's runs play it (bank 2's room, ADR 0251).
#[inline(never)]
#[allow(clippy::too_many_arguments)]
pub fn solo(
    o: &mut VarOsc,
    run: SoloRun,
    solo: Solo,
    out: &mut [f32; BLOCK_SIZE],
    mk: &mut [[f32; 2]; BLOCK_SIZE],
    r: &Ramps,
    jit: &mut f32,
    stab: &mut Stability,
    jd: f32,
) -> bool {
    let mut draw = || 1.0 + jd * stab.jitter_word() as f32;
    let d = &mut draw;
    macro_rules! play {
        ($a:ty, $b:ty, $rec:literal, $sync:literal, $flip:literal) => {
            o.render_solo::<$a, $b, BLOCK_SIZE, $rec, $sync, $flip>(
                solo, out, mk, r, jit, d, &ZEROS,
            )
        };
    }
    match (run, solo) {
        (SoloRun::Osc1, Solo::Saw) => play!(Saw, PulseQuiet, true, false, true),
        (SoloRun::Osc2, Solo::Saw) => play!(Saw, PulseQuiet, false, false, false),
        _ => return false,
    }
    true
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
    o.run_in::<A, B, N, REC, SYNC, FLIP, MOD, PH, false>(blk, span, r, out, mk, jit, draw, m, k);
}
