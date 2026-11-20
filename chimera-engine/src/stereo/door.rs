//! The stereo's table (ADR 0294): `kern`'s two blocks link into an S4
//! group, reached through `StereoDoor` at its own address a stride after
//! the meters' (`STEREO_DOOR_ADDR`). Without `bank2` it is bank 1's,
//! called directly. A closed door plays every note centred and every
//! Part's side as panned.

use crate::dsp::drum::types::SampleRate;
use crate::hw::{BLOCK_SIZE, MAX_PARTS};
use crate::instrument::{PartAudio, Sides};

use super::{PartWidths, VoicePan};

/// A note's place as the table carries it (`kern::pan`).
pub type PanRun = fn(&mut [f32; BLOCK_SIZE], &mut Option<[f32; BLOCK_SIZE]>, VoicePan, VoicePan);
/// The Parts' WIDTHs as the table carries them (`kern::widths`).
pub type WidthsRun = fn(&mut PartWidths, &mut Sides, &[PartAudio; MAX_PARTS], SampleRate);

/// `StereoDoor`'s note entry: called, never handed out.
#[repr(transparent)]
pub struct PanEntry(PanRun);

impl PanEntry {
    /// `x` placed from `from` to `to`, its side into `side`.
    #[inline(always)]
    pub fn run(
        &self,
        x: &mut [f32; BLOCK_SIZE],
        side: &mut Option<[f32; BLOCK_SIZE]>,
        from: VoicePan,
        to: VoicePan,
    ) {
        (self.0)(x, side, from, to)
    }
}

/// `StereoDoor`'s Part entry: called, never handed out.
#[repr(transparent)]
pub struct WidthsEntry(WidthsRun);

impl WidthsEntry {
    #[inline(always)]
    pub fn run(
        &self,
        w: &mut PartWidths,
        sides: &mut Sides,
        parts: &[PartAudio; MAX_PARTS],
        sample_rate: SampleRate,
    ) {
        (self.0)(w, sides, parts, sample_rate)
    }
}

/// `StereoDoor`'s mark: a table at the slot that isn't this one fails the
/// boot's door check (`door_ok`).
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Magic(u32);

const MAGIC: Magic = Magic(0xB2E0_0294);

/// The stereo's table, at its own slot (`STEREO_DOOR_ADDR`, `bank2.x`).
#[repr(C)]
pub struct StereoDoor {
    magic: Magic,
    pan: PanEntry,
    widths: WidthsEntry,
}

impl StereoDoor {
    const OWN: StereoDoor = StereoDoor {
        magic: MAGIC,
        pan: PanEntry(super::kern::pan),
        widths: WidthsEntry(super::kern::widths),
    };

    #[inline(always)]
    pub fn pan(&self) -> &PanEntry {
        &self.pan
    }

    #[inline(always)]
    pub fn widths(&self) -> &WidthsEntry {
        &self.widths
    }
}

#[cfg(feature = "bank2")]
// SAFETY: `bank2.x` links `STEREO_DOOR`, a `StereoDoor`, at
// `STEREO_DOOR_ADDR` in every firmware build with `bank2` (its assert,
// `__bank2_stereo_door`).
const SLOT: crate::bank2::DoorSlot<StereoDoor> =
    unsafe { crate::bank2::DoorSlot::new(crate::bank2::STEREO_DOOR_ADDR) };

#[cfg(feature = "bank2")]
#[unsafe(link_section = ".bank2.door.stereo")]
#[used]
static STEREO_DOOR: StereoDoor = StereoDoor::OWN;

/// The stereo's table in judged door `j` answers (the boot's door check,
/// run before the door opens).
#[cfg(feature = "bank2")]
pub fn door_ok(j: &crate::bank2::Judged) -> bool {
    j.with_slot(SLOT, |d| d.magic == MAGIC)
}

/// `f` on the stereo's table: bank 2's through the door on the unit,
/// `None` closed; this build's own elsewhere. A host test under `bank2`
/// answers as the unit does: only while the door cell is open.
#[inline(always)]
pub fn through<R>(f: impl FnOnce(&StereoDoor) -> R) -> Option<R> {
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
        Some(f(&StereoDoor::OWN))
    }
}
