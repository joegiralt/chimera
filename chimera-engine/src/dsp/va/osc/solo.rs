//! VA's saw alone on its anchor behind bank 2's door (ADR 0283). With
//! WAVE held on ⅔, OSC 1 and, but as SYNC's slave, OSC 2 play one run of
//! the saw, the pulse quiet (`OnAnchor`), instead of the pair's. `play`
//! holds both runs, inlined, so with `bank2` the module links into an S4
//! group whole (`bank2.x`, `*3osc4solo*`), reached through `SoloDoor` at
//! its own address at the head of the S4 groups: the door's table has no
//! room. Without `bank2` it is bank 1's, called
//! directly. `VarOsc::on_anchor`, `block` and the pair's runs stay in
//! bank 1.

use chimera_hal::BLOCK_SIZE;

use super::OnAnchor;
use crate::dsp::va::stability::{JitterScale, Stability, WrapJitter};

/// Which plain instance a solo plays: two of the engine's three
/// (`VaEngine::oscs`). SYNC's slave plays its pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoloRun {
    /// OSC 1, marking its wraps and the sub's flips.
    Osc1,
    /// OSC 2 as `Osc2Run::Plain` plays it: every plain MODE but SYNC,
    /// whose slave plays its pair.
    Osc2,
}

/// A solo render as the table carries it: its instance, the proof (the
/// oscillator and its ramps), its block, its marks, its wrap's jitter,
/// STABILITY and the jitter's scale.
pub type SoloRender = for<'a> fn(
    SoloRun,
    OnAnchor<'a>,
    &mut [f32; BLOCK_SIZE],
    &mut [[f32; 2]; BLOCK_SIZE],
    &mut WrapJitter,
    &mut Stability,
    JitterScale,
);

/// A solo render as `SoloDoor` holds it: called, never handed out.
#[repr(transparent)]
pub struct SoloEntry(SoloRender);

impl SoloEntry {
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        run: SoloRun,
        at: OnAnchor<'_>,
        out: &mut [f32; BLOCK_SIZE],
        mk: &mut [[f32; 2]; BLOCK_SIZE],
        jit: &mut WrapJitter,
        stab: &mut Stability,
        js: JitterScale,
    ) {
        (self.0)(run, at, out, mk, jit, stab, js)
    }
}

/// `SoloDoor`'s mark: a table at the slot that isn't this one fails the
/// boot's door check (`door_ok`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Magic(u32);

const MAGIC: Magic = Magic(0xB2A5_0283);

/// The solos' table, at its own slot after the door (`VA_SOLO_DOOR_ADDR`,
/// `bank2.x`), and AM's corrected render beside it (#660: the main door's
/// table has no room left). Its entries are private, called only through
/// `through` and `through_am`.
#[repr(C)]
pub struct SoloDoor {
    magic: Magic,
    solo: SoloEntry,
    am: crate::dsp::va::AmRender,
}

impl SoloDoor {
    const OWN: SoloDoor = SoloDoor {
        magic: MAGIC,
        solo: SoloEntry(play),
        am: crate::dsp::va::VaEngine::render_am_kinks,
    };

    /// The plain renders' solo (`play`).
    #[inline(always)]
    pub fn solo(&self) -> &SoloEntry {
        &self.solo
    }

    /// AM's corrected render (`VaEngine::render_am_kinks`).
    #[inline(always)]
    pub(in crate::dsp::va) fn am(
        &self,
        e: &mut crate::dsp::va::VaEngine,
        out: &mut [f32; BLOCK_SIZE],
        l: &crate::dsp::va::Lanes,
        gain: crate::dsp::va::Gain,
    ) {
        (self.am)(e, out, l, gain)
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
        #[cfg(feature = "test-support")]
        if FORCE_PAIRS.with(core::cell::Cell::get) {
            return None;
        }
        Some(f(&SoloDoor::OWN))
    }
}

/// `f` on the table for AM's render: `through`'s, but never forced to
/// pairs (AM's oscillators are always its listing pairs).
#[inline(always)]
pub fn through_am<R>(f: impl FnOnce(&SoloDoor) -> R) -> Option<R> {
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

#[cfg(feature = "test-support")]
extern crate std;

#[cfg(feature = "test-support")]
std::thread_local! {
    /// Host tests only: on this thread the solos' table answers as closed,
    /// the door open (`ForcePairs`).
    static FORCE_PAIRS: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

/// Host tests only: while it lives, every plain block this thread renders
/// plays its pair, the door's other entries as ever. Its drop restores what
/// was before it, so guards nest; no other thread sees it.
#[cfg(feature = "test-support")]
#[must_use]
pub struct ForcePairs {
    /// The flag before it, put back on drop: guards nest.
    prev: bool,
    /// Its thread's: never sent to another (`!Send`).
    _thread: core::marker::PhantomData<*const ()>,
}

#[cfg(feature = "test-support")]
impl ForcePairs {
    pub fn engage() -> ForcePairs {
        ForcePairs {
            prev: FORCE_PAIRS.with(|f| f.replace(true)),
            _thread: core::marker::PhantomData,
        }
    }
}

#[cfg(feature = "test-support")]
impl Drop for ForcePairs {
    fn drop(&mut self) {
        FORCE_PAIRS.with(|f| f.set(self.prev));
    }
}

/// A plain render's solo: OSC 1's instance (`<true, true>`, recording,
/// flipping) or OSC 2's (`<false, false>`), the saw with the pulse quiet.
#[inline(never)]
#[allow(clippy::too_many_arguments)]
fn play(
    run: SoloRun,
    at: OnAnchor<'_>,
    out: &mut [f32; BLOCK_SIZE],
    mk: &mut [[f32; 2]; BLOCK_SIZE],
    jit: &mut WrapJitter,
    stab: &mut Stability,
    js: JitterScale,
) {
    let d = &mut || js.draw(stab);
    match run {
        SoloRun::Osc1 => at.render::<true, true>(out, mk, jit, d),
        SoloRun::Osc2 => at.render::<false, false>(out, mk, jit, d),
    }
}
