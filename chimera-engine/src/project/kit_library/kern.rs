//! Bank 2's side of `KitDoor` (ADR 0305; `bank2.x` links this module
//! there): a kit's library records and its mark.

use chimera_hal::store::StoreError;

use crate::dsp::drum::types::Kit;
use crate::params::EngineType;
use crate::storage::{FileError, RecordTag, RecordWriter, records_crc};

use super::super::codec::{get_kit, get_kit_sound, put_kit, put_kit_sounds};
use super::super::{PartId, Project};
use super::{KitMark, KitSeen};

#[inline(never)]
#[optimize(size)]
pub(super) fn put(kit: &Kit, w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    put_kit(w, Some(kit))?;
    put_kit_sounds(w, Some(kit))
}

/// A kit's CRC: its records', so two kits a card round trip can't tell
/// apart are one. `None` is `Kit::DEFAULT`'s.
#[inline(never)]
#[optimize(size)]
fn kit_crc(kit: Option<&Kit>) -> u32 {
    records_crc(&[], |w| {
        put_kit(w, kit)?;
        put_kit_sounds(w, kit)
    })
}

/// A payload of the wrong length, or a pad there is none of, is
/// `Bounds`. A second `Kit` or pad's `KitSound` lands over the first.
#[inline(never)]
#[optimize(size)]
pub(super) fn read(tag: RecordTag, p: &[u8], kit: Option<&mut Kit>) -> Result<(), FileError> {
    match tag {
        RecordTag::Kit => get_kit(p, kit).map(drop).ok_or(FileError::Bounds),
        RecordTag::KitSound => {
            let (pad, sound, _) = get_kit_sound(p).ok_or(FileError::Bounds)?;
            if let Some(k) = kit {
                *k.pad_mut(pad).sound_mut() = sound;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[optimize(size)]
fn is_drum(p: &Project, id: PartId) -> bool {
    p.part(id).sound.engine() == EngineType::Drum
}

#[inline(never)]
#[optimize(size)]
pub(super) fn land(p: &mut Project, id: PartId) {
    if is_drum(p, id) {
        p.kits.land(id);
        settle(p, id);
    }
}

#[inline(never)]
#[optimize(size)]
pub(super) fn settle(p: &mut Project, id: PartId) {
    let crc = kit_crc(Some(p.kits.kit(id)));
    p.kits.set_mark(id, KitMark::At(crc));
}

#[inline(never)]
#[optimize(size)]
pub(super) fn seen(p: &Project, id: PartId) -> KitSeen {
    if !is_drum(p, id) {
        return KitSeen::Unread;
    }
    let crc = kit_crc(Some(p.kits.kit(id)));
    let mark = match p.kits.mark(id) {
        KitMark::Init => kit_crc(None),
        KitMark::At(c) => c,
    };
    if crc == mark {
        KitSeen::Kept(crc)
    } else {
        KitSeen::Edited(crc)
    }
}
