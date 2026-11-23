// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bank 2's side of `KitDoor` (ADR 0305; `bank2.x` links this module
//! there): a kit's library records, its CRC and its mark.

use chimera_hal::store::StoreError;

use crate::block::DiskCode;
use crate::dsp::drum::types::{Kit, PadId, PadSet, Tonal};
use crate::params::EngineType;
use crate::storage::{Event, FileError, ReadTag, RecordTag, RecordWriter, records_crc};

use super::super::codec::{KitRead, fill_pad_sounds, put_kit, put_kit_sounds};
use super::super::{PartId, Project};
use super::{FileKit, KitCrc, KitFound, KitMark, KitScan, KitSeen};

/// `kit`'s records, and their CRC: the mark of what was written.
#[inline(never)]
#[optimize(size)]
pub(super) fn put(kit: &Kit, w: &mut RecordWriter<'_>) -> Result<KitCrc, StoreError> {
    put_kit(w, Some(kit))?;
    put_kit_sounds(w, Some(kit))?;
    Ok(kit_crc(Some(kit)))
}

/// `None` is `Kit::DEFAULT`'s.
#[inline(never)]
#[optimize(size)]
fn kit_crc(kit: Option<&Kit>) -> KitCrc {
    KitCrc(records_crc(&[], |w| {
        put_kit(w, kit)?;
        put_kit_sounds(w, kit)
    }))
}

/// `kit` back to `Kit::DEFAULT`, in place.
#[optimize(size)]
fn reset(kit: &mut Kit) {
    for p in PadId::ALL {
        *kit.pad_mut(p) = Kit::default_pad(p);
    }
    kit.set_tonal(Tonal::Off);
}

/// A kit record belongs to a DRUM Sound's file only, after its `Engine`.
#[optimize(size)]
fn in_drum(scan: &KitScan) -> Result<(), FileError> {
    if scan.drum {
        Ok(())
    } else {
        Err(FileError::Corrupt)
    }
}

/// Every tag is named: a new one is weighed here before it compiles.
#[inline(never)]
#[optimize(size)]
pub(super) fn scan(
    scan: &mut KitScan,
    e: &Event<'_>,
    kit: Option<&mut Kit>,
) -> Result<(), FileError> {
    let (tag, p) = match *e {
        Event::Header(_) => {
            *scan = KitScan::NEW;
            if let Some(k) = kit {
                reset(k);
            }
            return Ok(());
        }
        // The Sound's check skips or refuses it.
        Event::Record(ReadTag::Unknown(_), _) => return Ok(()),
        Event::Record(ReadTag::Known(tag), p) => (tag, p),
    };
    match tag {
        RecordTag::Engine => {
            scan.drum = p == [EngineType::Drum.disk_code()];
            Ok(())
        }
        RecordTag::Kit => {
            in_drum(scan)?;
            scan.lossy |= scan.read.kit(p, kit)?;
            Ok(())
        }
        RecordTag::KitSound => {
            in_drum(scan)?;
            scan.lossy |= scan.read.sound(p, kit)?;
            Ok(())
        }
        // The Sound's, or a project's the Sound's check refuses.
        RecordTag::Block
        | RecordTag::Registry
        | RecordTag::ModDests
        | RecordTag::Routes
        | RecordTag::LastProject
        | RecordTag::Slot
        | RecordTag::Part
        | RecordTag::Fx
        | RecordTag::Origin
        | RecordTag::Tempo
        | RecordTag::OrbitBlock
        | RecordTag::OrbitNotes
        | RecordTag::OrbitChord
        | RecordTag::Orbit
        | RecordTag::OrbitScale
        | RecordTag::PoolSource
        | RecordTag::SourceRef
        | RecordTag::SourceHead
        | RecordTag::SourceData
        | RecordTag::SourceZones
        | RecordTag::ZoneTune
        | RecordTag::TapRoutes
        | RecordTag::Build
        | RecordTag::Note
        | RecordTag::MidiMap
        | RecordTag::Bindings
        | RecordTag::PerfRoutes
        | RecordTag::Controller
        | RecordTag::MapSection
        | RecordTag::MapRows
        | RecordTag::Scala => Ok(()),
    }
}

/// `KitSound`s with no `Kit` are a kit in part: `Corrupt`.
#[inline(never)]
#[optimize(size)]
pub(super) fn found(scan: &KitScan) -> Result<KitFound, FileError> {
    let r: &KitRead = &scan.read;
    let kit = match (r.has_kit(), r.pads() == PadSet::EMPTY) {
        (true, _) => FileKit::Whole(r.pads()),
        (false, true) => FileKit::Absent,
        (false, false) => return Err(FileError::Corrupt),
    };
    Ok(KitFound {
        kit,
        lossy: scan.lossy,
    })
}

/// After Part `id`'s Sound loaded from the library: a DRUM Sound brings
/// its file's kit (the scratch, its missing pad sounds from the Sound,
/// ADR 0269), which the library holds, or the default kit if its file
/// has none. Another engine's leaves the Part's kit: its file can hold
/// none (`scan` refuses one).
#[inline(never)]
#[optimize(size)]
pub(super) fn land(p: &mut Project, id: PartId, found: KitFound) {
    match (p.part(id).sound.engine(), found.kit) {
        (EngineType::Drum, FileKit::Whole(own)) => {
            p.kits.land(id);
            let i = id.index();
            fill_pad_sounds(p.kits.kit_mut(id), own, &p.perf.parts[i].sound.params);
            let crc = kit_crc(Some(p.kits.kit(id)));
            p.kits.set_mark(id, KitMark::held(crc));
        }
        (EngineType::Drum, FileKit::Absent) => reset(p.kits.kit_mut(id)),
        (
            EngineType::Algo | EngineType::Modal | EngineType::Tape | EngineType::Va,
            FileKit::Absent | FileKit::Whole(_),
        ) => {}
    }
}

#[inline(never)]
#[optimize(size)]
pub(super) fn seen(p: &Project, id: PartId) -> KitSeen {
    let crc = kit_crc(Some(p.kits.kit(id)));
    if crc == kit_crc(None) {
        KitSeen::Default(crc)
    } else if p.kits.mark(id) == KitMark::held(crc) {
        KitSeen::InLibrary(crc)
    } else {
        KitSeen::Unsaved(crc)
    }
}
