//! The pool on the arena against a card that fails (ADR 0087, Review Focus
//! 1): a load's second pass cut short leaves NEW's arena, not half of one;
//! a 128-slot project save cut or refused at every block write keeps a
//! generation, cross-links nothing and loses no cluster but the one #359
//! leaves.

#[path = "common/ab.rs"]
mod ab;
#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;

use ab::{copy, op, slot};
use chimera_core::block::Block;
use chimera_core::part::PartParams;
use chimera_core::project::arena::ARENA_BYTES;
use chimera_core::project::test_support::{full, full_dense_slots, same, save_at};
use chimera_core::project::{
    LoadLink, PartId, Project, ProjectDecoder, ProjectNote, ProjectSource, ReplaceGuard,
    TemplateCrc, list_projects, load_project, project_crc, project_file,
};
use chimera_core::storage::{Card, InPlaceError, ProjectId, load_ab_in_place};
use chimera_fat::FatStore;
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use core::ops::ControlFlow;
use image::{Cut, RamDisk, fat_check, with_clusters};
use probe::{Log, Probed, log, probed};
use std::rc::Rc;

/// FAT16, 512 B clusters: a 128-slot project spans many.
const CLUSTERS: u32 = 4_200;

fn id() -> ProjectId {
    ProjectId::new(1).unwrap()
}

/// What a test's saves start from: `full()`, or a dense variant.
type Base = fn() -> (Box<Project>, TemplateCrc);

/// `full_dense_slots(128)`: 175 KB, its chain crossing FAT sectors.
fn dense_128() -> (Box<Project>, TemplateCrc) {
    full_dense_slots(128)
}

/// `base`, all 128 slots filled, told apart by Part 1's level.
fn project_gen(base: Base, n: u32) -> Box<Project> {
    let (mut p, _) = base();
    p.edit_part(PartId::ALL[0])
        .mix
        .set(PartParams::LEVEL, 0.1 * n as f32);
    p
}

fn saved(note: &ProjectNote) -> bool {
    matches!(note, ProjectNote::Saved(_))
}

/// Save `n` over the pair as listed, or to it fresh.
fn save_gen<S: Store>(base: Base, s: &mut S, n: u32) -> ProjectNote {
    save_at(&mut Card::new(), s, &mut project_gen(base, n), id().get())
}

/// The pair loaded in place into a fresh project, as after a power cycle.
fn load_in_place<D: embedded_sdmmc::BlockDevice<Error = image::DiskError>>(
    disk: &Rc<D>,
) -> Result<Box<Project>, InPlaceError> {
    let (mut q, _) = Project::boxed();
    let mut s = probed(disk);
    Card::new()
        .run(&mut s, |s, r| {
            load_ab_in_place(
                s,
                r,
                project_file(id()),
                &mut ProjectDecoder::new_for_test(&mut q),
            )
        })
        .and_then(|o| o.result)?;
    Ok(q)
}

/// Loads a 2 or a 3 whole, whichever the card kept.
fn loads_two_or_three<D: embedded_sdmmc::BlockDevice<Error = image::DiskError>>(
    base: Base,
    what: &str,
    disk: &Rc<D>,
) {
    let got = load_in_place(disk).unwrap_or_else(|e| panic!("{what}: {e:?}"));
    let three = project_gen(base, 3);
    if project_crc(&got) == project_crc(&three) {
        same(&three, &got);
    } else {
        same(&project_gen(base, 2), &got);
    }
}

// A load's second pass, cut short.

/// What goes wrong once `after` chunks of the second read have arrived.
#[derive(Clone, Copy, Debug)]
enum Fault {
    /// Every read errs from here.
    Reads,
    /// The card leaves the slot.
    Absent,
}

/// Passes chunks on; trips `fault` on the probe once `after` have gone.
struct Trip<'a> {
    inner: &'a mut dyn ReadSink,
    log: &'a Log,
    fault: Fault,
    after: u32,
    seen: u32,
}

impl ReadSink for Trip<'_> {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        self.inner.begin(len)
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        let flow = self.inner.chunk(bytes);
        self.seen += 1;
        if self.seen == self.after {
            match self.fault {
                Fault::Reads => self.log.fail_reads.set(u32::MAX),
                Fault::Absent => self.log.absent.set(true),
            }
        }
        flow
    }
}

/// A probed card whose second read of any one file trips `fault`: pass 2.
struct MidPass2 {
    inner: Probed<RamDisk>,
    fault: Fault,
    after: u32,
    first: Option<FileName>,
    tripped: bool,
}

impl Store for MidPass2 {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.inner.mount()
    }

    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.inner.list(vol, dir, f)
    }

    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        if self.first.is_none() {
            self.first = Some(file);
        } else if self.first == Some(file) && !self.tripped {
            self.tripped = true;
            let log = self.inner.device().log.clone();
            let mut trip = Trip {
                inner: sink,
                log: &log,
                fault: self.fault,
                after: self.after,
                seen: 0,
            };
            return self.inner.read(vol, file, &mut trip);
        }
        self.inner.read(vol, file, sink)
    }

    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.inner.write(vol, file, body)
    }

    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.inner.delete(vol, file)
    }

    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.inner.make_dir(vol, dir)
    }
}

/// A device error or a pulled card partway through pass 2: the load fails
/// back to NEW, and the arena holds NEW's Sounds and nothing else.
#[test]
fn load_device_error_leaves_new_arena() {
    let base: Base = full;
    let disk = Rc::new(with_clusters(CLUSTERS, 0xC0DE));
    assert!(saved(&save_gen(base, &mut probed(&disk), 1)));
    let (new, t) = Project::boxed();
    let new_used = new.pool().used();
    for fault in [Fault::Reads, Fault::Absent] {
        for after in [1, 8, 30] {
            let what = format!("{fault:?} after {after} chunks");
            // A RAM project nothing like NEW, its edits unsaved.
            let (mut q, _) = full();
            let mut card = Card::new();
            let mut entry = None;
            let listed = list_projects(&mut card, &mut probed(&disk), &mut |e| entry = Some(e));
            assert_eq!(listed.note, None, "{what}");
            let e = entry.expect("listed");
            // The load's reads from here: pass 1, then pass 2.
            let mut s = MidPass2 {
                inner: probed(&disk),
                fault,
                after,
                first: None,
                tripped: false,
            };
            let src = ProjectSource::File {
                id: e.id,
                vol: e.vol,
            };
            let go =
                ReplaceGuard::check(&q, t, src).unwrap_or_else(|n| n.into_pending().anyway(&q));
            let out = load_project(&mut card, &mut s, &mut q, go, &LoadLink::new());
            assert!(s.tripped, "{what}: pass 2 ran");
            assert!(
                matches!(out.note, Some(ProjectNote::LoadFailed(_))),
                "{what}: {:?}",
                out.note
            );
            assert_eq!(project_crc(&q), t.get(), "{what}: NEW");
            same(&new, &q);
            assert_eq!(q.pool().used(), new_used, "{what}: NEW's arena");
            assert_eq!(
                q.pool().free().bytes() as usize,
                ARENA_BYTES - new_used.bytes() as usize,
                "{what}: the rest is gap"
            );
        }
    }
}

// A 128-slot project save, cut or refused at every block write.

/// A card holding saves 1 and 2 of the 128-slot project.
fn two_saves(base: Base) -> RamDisk {
    let mut s = FatStore::new(with_clusters(CLUSTERS, 0xC0DE));
    for n in 1..=2 {
        let note = save_gen(base, &mut s, n);
        assert!(saved(&note), "save {n}: {note:?}");
    }
    let RamDisk(blocks) = s.device();
    RamDisk(blocks.clone())
}

/// The blocks save 3 writes on `base`, in order.
fn save_three_writes(base: Base, card: &RamDisk) -> Vec<u32> {
    let disk = Rc::new(copy(card));
    let mut s = probed(&disk);
    assert!(saved(&save_gen(base, &mut s, 3)));
    log(&s).writes.borrow().iter().map(|&(b, _)| b).collect()
}

/// The directory blocks holding the pair's entries: where a `put_entry`
/// writes.
fn entry_blocks(disk: &RamDisk) -> Vec<u32> {
    let r = fat_check(disk);
    let stem = id().stem();
    r.entries
        .iter()
        .filter(|e| e.name[..8] == stem[..])
        .map(|e| e.block)
        .collect()
}

/// Power cut at each block save 3 writes: the save fails, nothing is
/// cross-linked or over its chain, the card loads save 2 or 3 whole, and
/// the next save lands.
#[test]
fn project_save_cut_at_every_write() {
    cut_at_every_write(full);
}

/// The same on a 175 KB project whose chain crosses FAT sectors (#362 is
/// a device error, not a cut).
#[test]
fn dense_project_save_cut_at_every_write() {
    cut_at_every_write(dense_128);
}

fn cut_at_every_write(base: Base) {
    let before = two_saves(base);
    let p2 = project_gen(base, 2);
    assert_eq!(p2.pool().sound_count(), 128);
    let blocks = save_three_writes(base, &before);
    let chains = fat_check(&before).entries.len();
    assert!(blocks.len() > 60, "{} writes", blocks.len());
    for k in 0..blocks.len() {
        let what = format!("cut at write {k} of {}", blocks.len());
        let cut = slot(copy(&before));
        cut.cut.set(Cut::After(k as u32));
        let note = op(&mut probed(&cut), |s, _| {
            Ok::<_, StoreError>(save_gen(base, s, 3))
        })
        .unwrap();
        assert!(!saved(&note), "{what}: {note:?}");
        cut.cut.set(Cut::Never);
        let r = fat_check(&cut.inner);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty(),
            "{what}: {r:?}"
        );
        assert!(r.entries.len() >= chains, "{what}: an entry lost");
        loads_two_or_three(base, &what, &cut);
        assert!(
            saved(&save_gen(base, &mut probed(&cut), 4)),
            "{what}: next save"
        );
        same(&project_gen(base, 4), &load_in_place(&cut).unwrap());
    }
}

/// After a fault: the next change on the card, as any save would make,
/// settles a chain or a FAT 2 sector the card refused; then nothing is
/// cross-linked or over its chain, and no cluster is lost unless the
/// failed write was the entry's (#359 leaves that chain for #360).
fn assert_no_leak(
    base: Base,
    what: &str,
    s: &mut Probed<RamDisk>,
    disk: &Rc<RamDisk>,
    failed: u32,
    entries: &[u32],
    lost_before: &[u32],
) {
    let vol = s.mount().unwrap();
    let next = FileName::new(Dir::Chimera, b"NEXT", b"BIN").unwrap();
    s.write(vol, next, &mut |w| w.put(b"x")).unwrap();
    let r = fat_check(disk);
    assert!(
        r.cross_linked.is_empty() && r.short.is_empty(),
        "{what}: {r:?}"
    );
    if !entries.contains(&failed) {
        assert_eq!(r.lost, lost_before, "{what}: lost");
    }
    loads_two_or_three(base, what, disk);
}

/// A device error at each block save 3 writes, the card still powered:
/// every write to that block refused, then a write that lands and errs.
#[test]
fn project_save_device_error_no_leak() {
    let base: Base = full;
    let card = two_saves(base);
    let blocks = save_three_writes(base, &card);
    let entries = entry_blocks(&card);
    let lost_before = fat_check(&card).lost;
    assert!(lost_before.is_empty(), "{lost_before:?}");
    for (k, &block) in blocks.iter().enumerate() {
        let what = format!("write {k} of {} (block {block})", blocks.len());

        let disk = Rc::new(copy(&card));
        let mut s = probed(&disk);
        log(&s).fail_write.set(Some(block));
        let note = save_gen(base, &mut s, 3);
        assert!(!saved(&note), "fail_write {what}: {note:?}");
        log(&s).fail_write.set(None);
        let what = format!("fail_write {what}");
        assert_no_leak(base, &what, &mut s, &disk, block, &entries, &lost_before);

        let disk = Rc::new(copy(&card));
        let mut s = probed(&disk);
        log(&s).land_then_fail.set(Some(k));
        let _ = save_gen(base, &mut s, 3);
        log(&s).land_then_fail.set(None);
        let what = format!("land_then_fail {what}");
        assert_no_leak(base, &what, &mut s, &disk, block, &entries, &lost_before);
    }
}

/// A device error as a chain crosses into the next FAT sector, FAT 2's
/// write among them, strands nothing: `drop_chain` frees the new sector's
/// first cluster (#362), with no sweep.
#[test]
fn sector_crossing_fat2_error_loses_nothing() {
    let base: Base = full;
    let card = two_saves(base);
    let (mut big, _) = full_dense_slots(128);
    let body_writes = {
        let disk = Rc::new(copy(&card));
        let mut s = probed(&disk);
        assert!(saved(&save_at(
            &mut Card::new(),
            &mut s,
            &mut big,
            id().get()
        )));
        log(&s)
            .writes
            .borrow()
            .iter()
            .map(|&(b, _)| b)
            .collect::<Vec<_>>()
    };
    let entries = entry_blocks(&card);
    for (k, &block) in body_writes.iter().enumerate() {
        let disk = Rc::new(copy(&card));
        let mut s = probed(&disk);
        log(&s).fail_write.set(Some(block));
        let (mut big, _) = full_dense_slots(128);
        let _ = save_at(&mut Card::new(), &mut s, &mut big, id().get());
        log(&s).fail_write.set(None);
        let vol = s.mount().unwrap();
        let next = FileName::new(Dir::Chimera, b"NEXT", b"BIN").unwrap();
        s.write(vol, next, &mut |w| w.put(b"x")).unwrap();
        let r = fat_check(&disk);
        if !entries.contains(&block) {
            assert!(
                r.lost.is_empty(),
                "write {k} (block {block}): lost {:?}",
                r.lost
            );
        }
    }
}
