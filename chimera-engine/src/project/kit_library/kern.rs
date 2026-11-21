//! Bank 2's side of `KitDoor` (ADR 0305; `bank2.x` links this module
//! there): a kit's library records and its mark.

use chimera_hal::store::StoreError;

use crate::dsp::drum::types::{Kit, PadId, Tonal};
use crate::params::EngineType;
use crate::preset::Sound;
use crate::storage::{Event, FileError, ReadTag, RecordTag, RecordWriter, records_crc};

use super::super::codec::{get_kit, get_kit_sound, put_kit, put_kit_sounds};
use super::super::{PartId, Project};
use super::{KitMark, KitSeen};

#[inline(never)]
#[optimize(size)]
pub(super) fn put(s: &Sound, kit: &Kit, w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    if s.engine() != EngineType::Drum {
        return Ok(());
    }
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

/// The header: `kit` back to `Kit::DEFAULT`, so a file with no kit (a
/// Sound's own save, or one from before ADR 0305) leaves that. A kit
/// record's payload of the wrong length, or a pad there is none of, is
/// `Bounds`. A second `Kit` or pad's `KitSound` lands over the first.
#[inline(never)]
#[optimize(size)]
pub(super) fn read(e: &Event<'_>, kit: &mut Kit) -> Result<(), FileError> {
    match *e {
        Event::Header(_) => {
            for p in PadId::ALL {
                *kit.pad_mut(p) = Kit::default_pad(p);
            }
            kit.set_tonal(Tonal::Off);
            Ok(())
        }
        Event::Record(ReadTag::Known(RecordTag::Kit), p) => {
            get_kit(p, Some(kit)).map(drop).ok_or(FileError::Bounds)
        }
        Event::Record(ReadTag::Known(RecordTag::KitSound), p) => {
            let (pad, sound, _) = get_kit_sound(p).ok_or(FileError::Bounds)?;
            *kit.pad_mut(pad).sound_mut() = sound;
            Ok(())
        }
        Event::Record(..) => Ok(()),
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
pub(super) fn saved(p: &mut Project, id: PartId) {
    if is_drum(p, id) {
        settle(p, id);
    }
}

#[inline(never)]
#[optimize(size)]
pub(super) fn seen(p: &Project, id: PartId) -> KitSeen {
    if !is_drum(p, id) {
        return KitSeen::UNREAD;
    }
    let crc = kit_crc(Some(p.kits.kit(id)));
    let mark = match p.kits.mark(id) {
        KitMark::Init => kit_crc(None),
        KitMark::At(c) => c,
    };
    KitSeen::new(crc, crc != mark)
}
