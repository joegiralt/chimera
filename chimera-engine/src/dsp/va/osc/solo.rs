//! VA's saw alone on its anchor behind bank 2's door (ADR 0283). With
//! WAVE held on ⅔, OSC 1 and MIX's OSC 2 play one run of the saw, the
//! pulse quiet (`Solo`), instead of the pair's. `play` holds both runs,
//! inlined, so with `bank2` the module links into an S4 group whole
//! (`bank2.x`, `*3osc4solo*`), reached through `SoloDoor` at its own slot:
//! the door's table has no room. Without `bank2` it is bank 1's, called
//! directly. `Solo::at`, `block` and the pair's runs stay in bank 1.

use chimera_hal::BLOCK_SIZE;

use super::{PulseQuiet, Ramps, Saw, Solo, VarOsc, ZEROS};
use crate::dsp::va::stability::Stability;

/// Which plain instance a solo plays: two of the engine's three
/// (`VaEngine::oscs`). SYNC's slave plays its pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoloRun {
    /// OSC 1, marking its wraps and the sub's flips.
    Osc1,
    /// OSC 2, plain (every MODE but SYNC that runs the plain renders).
    Osc2,
}

/// A solo render as the table carries it: the oscillator, its instance and
/// solo, its block, its marks, its lanes and jitter, STABILITY (the
/// jitter's draws, `1 + jd · word`).
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
);

/// A solo render as `SoloDoor` holds it: called, never handed out.
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
    ) {
        (self.0)(o, run, solo, out, mk, r, jit, stab, jd)
    }
}

/// `SoloDoor`'s mark: a table at the slot that isn't this one fails the
/// boot's door check (`door_ok`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Magic(u32);

const MAGIC: Magic = Magic(0xB2A5_0283);

/// The solos' table, at its own slot after the door (`VA_SOLO_DOOR_ADDR`,
/// `bank2.x`). Its entry is private, called only through `through`.
#[repr(C)]
pub struct SoloDoor {
    magic: Magic,
    solo: SoloEntry,
}

impl SoloDoor {
    const OWN: SoloDoor = SoloDoor {
        magic: MAGIC,
        solo: SoloEntry(play),
    };

    /// The plain renders' solo (`play`).
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

/// A plain render's solo: OSC 1's instance (`<64, true, false, true>`) or
/// OSC 2's (`<64, false, false, false>`), the saw with the pulse quiet.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn play(
    o: &mut VarOsc,
    run: SoloRun,
    solo: Solo,
    out: &mut [f32; BLOCK_SIZE],
    mk: &mut [[f32; 2]; BLOCK_SIZE],
    r: &Ramps,
    jit: &mut f32,
    stab: &mut Stability,
    jd: f32,
) {
    let mut draw = || 1.0 + jd * stab.jitter_word() as f32;
    let d = &mut draw;
    match (run, solo) {
        (SoloRun::Osc1, Solo::Saw) => o
            .render_solo::<Saw, PulseQuiet, BLOCK_SIZE, true, false, true>(
                solo, out, mk, r, jit, d, &ZEROS,
            ),
        (SoloRun::Osc2, Solo::Saw) => o
            .render_solo::<Saw, PulseQuiet, BLOCK_SIZE, false, false, false>(
                solo, out, mk, r, jit, d, &ZEROS,
            ),
    }
}
