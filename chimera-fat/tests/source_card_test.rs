//! Sources on the card (ADR 0087, ADR 0093): write-once `/CHIMERA/TAPES`
//! files, sealed once verified, against power cuts and device errors at
//! every block; and how a project load reads them (Review Focus 4).

#[path = "common/ab.rs"]
mod ab;
#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;

use ab::{copy, slot};
use chimera_core::dsp::tape_engine::read::{DESC_LEN, Descriptor, Shape};
use chimera_core::name::SoundName;
use chimera_core::project::test_support::{FailOnSecondRead, save_at};
use chimera_core::project::{
    LoadLink, LoadOutcome, PartId, Project, ProjectNote, ProjectSource, ReplaceGuard, SourceRef,
    load_project, project_file,
};
use chimera_core::storage::{
    Card, LoadError, ProjectId, SaveError, Side, SourceHead, Wrote, read_source_into, source_crc,
    source_file, write_source,
};
use chimera_hal::store::{Dir, Store, StoreError, VolumeId};
use embedded_sdmmc::BlockDevice;
use image::{Cut, DiskError, RamDisk, fat_check, with_clusters};
use probe::{log, probed, swept_clean};
use std::rc::Rc;

/// FAT16, 512 B clusters: a source spans a dozen.
const CLUSTERS: u32 = 4_200;
/// 6 000 B of samples.
const FRAMES: u32 = 4_000;

fn source(seed: u8, frames: u32) -> (SourceHead, Vec<u8>) {
    let data: Vec<u8> = (0..3 * frames.div_ceil(2) as usize)
        .map(|i| (i as u32).wrapping_mul(29).wrapping_add(seed as u32) as u8)
        .collect();
    let head = SourceHead {
        sref: SourceRef {
            crc: source_crc(Shape::Plain, 1, &data),
            frames,
            rate: 24_000,
        },
        shape: Shape::Plain,
        count: 1,
        root: 6_900,
        period_q16: 0,
        name: SoundName::new("TAPE").unwrap(),
    };
    (head, data)
}

fn held(head: &SourceHead, data: &[u8]) -> Vec<u8> {
    let d = Descriptor {
        shape: head.shape,
        count: head.count,
        rate: head.sref.rate,
        head_frames: 0,
    };
    let mut v = d.to_bytes().to_vec();
    v.extend_from_slice(data);
    v
}

fn write_on<S: Store>(s: &mut S, head: &SourceHead, data: &[u8]) -> Result<Wrote, SaveError> {
    Card::new()
        .run(s, |s, r| write_source(s, r, head, data))
        .and_then(|o| o.result)
}

fn read_on<S: Store>(s: &mut S, sref: SourceRef) -> Result<Vec<u8>, LoadError> {
    let mut out = vec![0; DESC_LEN + 3 * sref.frames.div_ceil(2) as usize];
    let n = Card::new()
        .run(s, |s, r| read_source_into(s, r, sref, &mut out))
        .and_then(|o| o.result)?;
    out.truncate(n);
    Ok(out)
}

fn clean(what: &str, disk: &RamDisk) {
    let r = fat_check(disk);
    assert!(
        r.cross_linked.is_empty() && r.short.is_empty(),
        "{what}: {r:?}"
    );
}

/// The blocks `write_source` writes on a copy of `base`, in order.
fn source_writes(base: &RamDisk, head: &SourceHead, data: &[u8]) -> Vec<u32> {
    let disk = Rc::new(copy(base));
    let mut s = probed(&disk);
    assert_eq!(write_on(&mut s, head, data), Ok(Wrote::New));
    log(&s).writes.borrow().iter().map(|&(b, _)| b).collect()
}

/// A card holding `/CHIMERA/PROJECTS/P0000001` on one source, `head`'s,
/// and no source file yet.
fn with_project(head: &SourceHead) -> RamDisk {
    let mut s = chimera_fat::FatStore::new(with_clusters(CLUSTERS, 0x5EC));
    let (mut p, _) = Project::boxed();
    p.source_absent(head.sref).unwrap();
    p.edit_part(PartId::ALL[0]).sound.source = Some(head.sref);
    let note = save_at(&mut Card::new(), &mut s, &mut p, 1);
    assert!(matches!(note, ProjectNote::Saved(_)), "{note:?}");
    let RamDisk(blocks) = s.device();
    RamDisk(blocks.clone())
}

fn load<S: Store>(s: &mut S) -> (Box<Project>, LoadOutcome) {
    let (mut q, t) = Project::boxed();
    let vol = s.mount().unwrap();
    let id = ProjectId::new(1).unwrap();
    let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol }).expect("no prompt");
    let out = load_project(&mut Card::new(), s, &mut q, go, &LoadLink::new());
    (q, out)
}

/// Review Focus 4: a source write cut at every block reads as missing,
/// never as shorter audio, alone and through a project load. Only a cut
/// at the seal, after the file verified, leaves the whole source.
#[test]
fn truncated_source_reads_missing() {
    let (h, data) = source(1, FRAMES);
    let base = with_project(&h);
    let writes = source_writes(&base, &h, &data);
    assert!(writes.len() > 12, "{} writes", writes.len());
    let whole = held(&h, &data);
    for k in 0..writes.len() {
        let what = format!("cut at write {k} of {}", writes.len());
        let cut = slot(copy(&base));
        cut.cut.set(Cut::After(k as u32));
        assert!(write_on(&mut probed(&cut), &h, &data).is_err(), "{what}");
        cut.cut.set(Cut::Never);
        clean(&what, &cut.inner);
        let last = k == writes.len() - 1;
        match read_on(&mut probed(&cut), h.sref) {
            Ok(got) => {
                assert!(last, "{what}: read before its seal");
                assert_eq!(got, whole, "{what}");
            }
            Err(e) => assert!(!last, "{what}: {e:?}"),
        }
        let (q, out) = load(&mut probed(&cut));
        assert_eq!(out.note, None, "{what}");
        let id = q.pool().find_source(h.sref).expect("the ref");
        let e = q.pool().source(id).unwrap();
        if last {
            assert_eq!(e.held as usize, whole.len(), "{what}");
        } else {
            assert!(e.absent(), "{what}: {e:?}");
        }
    }
}

/// Review Focus 4: a card swapped between a load's two passes refuses
/// the load as before, and no source byte enters the arena.
#[test]
fn card_swap_between_passes_reads_missing() {
    let (h, data) = source(2, FRAMES);
    let disk = Rc::new(with_project(&h));
    assert_eq!(write_on(&mut probed(&disk), &h, &data), Ok(Wrote::New));
    let other = VolumeId {
        serial: 0xBAD,
        label: *b"OTHER      ",
    };
    let file = project_file(ProjectId::new(1).unwrap()).side(Side::A);
    let mut s = FailOnSecondRead::new(probed(&disk), file, StoreError::VolumeChanged(other));
    let (q, out) = load(&mut s);
    assert_eq!(s.reads(), 2, "pass 2 ran");
    assert!(
        matches!(out.note, Some(ProjectNote::LoadFailed(_))),
        "{:?}",
        out.note
    );
    assert_eq!(q.pool().find_source(h.sref), None);
    assert_eq!(q.pool().source_used().bytes(), 0);
    assert_eq!(q.pool().source_count(), 0);
    // The same card unswapped: the source loads whole.
    let (q, _) = load(&mut probed(&disk));
    let id = q.pool().find_source(h.sref).unwrap();
    assert_eq!(
        q.pool().source(id).unwrap().held as usize,
        held(&h, &data).len()
    );
}

/// The directory blocks holding `name`'s entry: where its `put_entry`
/// writes.
fn entry_block(disk: &RamDisk, name: &[u8; 11]) -> Option<u32> {
    fat_check(disk).entry(name).map(|e| e.block)
}

fn name83(h: &SourceHead) -> [u8; 11] {
    let mut n = [b' '; 11];
    n[..8].copy_from_slice(format!("{:08X}", h.sref.crc).as_bytes());
    n[8..].copy_from_slice(b"SRC");
    n
}

/// After a fault: nothing cross-linked or over its chain, and no cluster
/// lost unless the failed write was an entry's, the file's or `TAPES`'s
/// (#359 leaves that chain to
/// the sweep, #360, which then frees it); a retry writes the source whole.
fn assert_no_leak<D: BlockDevice<Error = DiskError>>(
    what: &str,
    disk: &Rc<D>,
    image: &RamDisk,
    failed: u32,
    entries: &[u32],
    h: &SourceHead,
    data: &[u8],
) {
    let r = fat_check(image);
    assert!(
        r.cross_linked.is_empty() && r.short.is_empty(),
        "{what}: {r:?}"
    );
    if !entries.contains(&failed) {
        assert_eq!(r.lost, Vec::<u32>::new(), "{what}: lost");
    } else {
        swept_clean(what, disk, image);
    }
    let again = write_on(&mut probed(disk), h, data);
    assert!(
        matches!(again, Ok(Wrote::New | Wrote::Replaced | Wrote::Present)),
        "{what}: {again:?}"
    );
    assert_eq!(
        read_on(&mut probed(disk), h.sref),
        Ok(held(h, data)),
        "{what}"
    );
    clean(what, image);
}

/// A device error at each block the write writes, the card still powered:
/// every write to that block refused, then a write that lands and errs.
#[test]
fn source_write_device_error_no_leak() {
    let (h, data) = source(3, FRAMES);
    let base = with_project(&h);
    let writes = source_writes(&base, &h, &data);
    let entries = {
        let done = Rc::new(copy(&base));
        write_on(&mut probed(&done), &h, &data).unwrap();
        [name83(&h), *b"TAPES      "].map(|n| entry_block(&done, &n).expect("the entry"))
    };
    assert!(entries.iter().all(|e| writes.contains(e)));
    assert_eq!(fat_check(&base).lost, Vec::<u32>::new());
    for (k, &block) in writes.iter().enumerate() {
        let what = format!("write {k} of {} (block {block})", writes.len());

        let disk = Rc::new(copy(&base));
        let mut s = probed(&disk);
        log(&s).fail_write.set(Some(block));
        assert!(write_on(&mut s, &h, &data).is_err(), "fail_write {what}");
        log(&s).fail_write.set(None);
        let w = format!("fail_write {what}");
        assert_no_leak(&w, &disk, &disk, block, &entries, &h, &data);

        let disk = Rc::new(copy(&base));
        let mut s = probed(&disk);
        log(&s).land_then_fail.set(Some(k));
        assert!(
            write_on(&mut s, &h, &data).is_err(),
            "land_then_fail {what}"
        );
        log(&s).land_then_fail.set(None);
        let w = format!("land_then_fail {what}");
        assert_no_leak(&w, &disk, &disk, block, &entries, &h, &data);
    }
}

/// A power cut at each block the write writes: nothing cross-linked or
/// over its chain. What a cut strands is the sweep's (#360,
/// private issue #360).
#[test]
fn source_write_power_cut_safe() {
    let (h, data) = source(4, FRAMES);
    let base = with_project(&h);
    let writes = source_writes(&base, &h, &data);
    for k in 0..writes.len() {
        let what = format!("cut at write {k} of {}", writes.len());
        let cut = slot(copy(&base));
        cut.cut.set(Cut::After(k as u32));
        assert!(write_on(&mut probed(&cut), &h, &data).is_err(), "{what}");
        cut.cut.set(Cut::Never);
        clean(&what, &cut.inner);
        swept_clean(&what, &cut, &cut.inner);
    }
}

/// A sealed source: `write_source` finds it Present and writes no block,
/// and a raw write to it is refused, the file unchanged.
#[test]
fn source_is_write_once() {
    let (h, data) = source(5, FRAMES);
    let disk = Rc::new(with_clusters(CLUSTERS, 0x5EC));
    assert_eq!(write_on(&mut probed(&disk), &h, &data), Ok(Wrote::New));
    let mut s = probed(&disk);
    assert_eq!(write_on(&mut s, &h, &data), Ok(Wrote::Present));
    assert_eq!(log(&s).writes.borrow().len(), 0, "no block written");
    let vol = s.mount().unwrap();
    let raw = s.write(vol, source_file(h.sref), &mut |w| w.put(b"over"));
    assert_eq!(raw, Err(StoreError::ReadOnly));
    assert_eq!(
        s.delete(vol, source_file(h.sref)),
        Err(StoreError::ReadOnly)
    );
    assert_eq!(log(&s).writes.borrow().len(), 0, "still none");
    assert_eq!(read_on(&mut s, h.sref), Ok(held(&h, &data)));
}

/// A sealed file that fails its check is Damaged, and nothing is written.
#[test]
fn sealed_damaged_source_is_refused() {
    let (h, data) = source(6, FRAMES);
    let disk = Rc::new(with_clusters(CLUSTERS, 0x5EC));
    assert_eq!(write_on(&mut probed(&disk), &h, &data), Ok(Wrote::New));
    // One sample flipped in the file's last data block.
    let e = fat_check(&disk).entry(&name83(&h)).cloned().unwrap();
    let last = *e.chain.last().unwrap();
    let block = fat_block_of(&disk, last);
    let mut b = disk.block(block);
    b[3] ^= 0x40;
    disk.0.borrow_mut()[block as usize] = b;
    let mut s = probed(&disk);
    assert_eq!(write_on(&mut s, &h, &data), Err(SaveError::Damaged));
    assert_eq!(log(&s).writes.borrow().len(), 0, "nothing written");
    assert!(read_on(&mut s, h.sref).is_err());
}

/// Cluster `c`'s first block, absolute (512 B clusters).
fn fat_block_of(disk: &RamDisk, c: u32) -> u32 {
    let l = image::layout_of(disk);
    image::PART_LBA + l.cluster_block(c).unwrap()
}

/// A write cut at each block, then `write_source` again: the torn file is
/// Replaced (New if no entry landed), verified and sealed. A cut at the
/// seal itself left a whole file: it is sealed as it is, Present, never
/// deleted.
#[test]
fn torn_source_is_replaced() {
    let (h, data) = source(7, FRAMES);
    let base = with_clusters(CLUSTERS, 0x5EC);
    let writes = source_writes(&base, &h, &data);
    let mut replaced = 0;
    for k in 0..writes.len() {
        let what = format!("cut at write {k} of {}", writes.len());
        let cut = slot(copy(&base));
        cut.cut.set(Cut::After(k as u32));
        assert!(write_on(&mut probed(&cut), &h, &data).is_err(), "{what}");
        cut.cut.set(Cut::Never);
        let had = fat_check(&cut.inner).entry(&name83(&h)).is_some();
        let whole = read_on(&mut probed(&cut), h.sref).is_ok();
        assert_eq!(
            whole,
            k == writes.len() - 1,
            "{what}: whole only cut at the seal"
        );
        let again = write_on(&mut probed(&cut), &h, &data);
        let want = match (whole, had) {
            (true, _) => Wrote::Present,
            (false, true) => Wrote::Replaced,
            (false, false) => Wrote::New,
        };
        assert_eq!(again, Ok(want), "{what}");
        replaced += usize::from(had && !whole);
        clean(&what, &cut.inner);
        let mut s = probed(&cut);
        assert_eq!(read_on(&mut s, h.sref), Ok(held(&h, &data)), "{what}");
        let vol = s.mount().unwrap();
        let raw = s.write(vol, source_file(h.sref), &mut |w| w.put(b"x"));
        assert_eq!(raw, Err(StoreError::ReadOnly), "{what}: sealed");
    }
    assert!(replaced > 0, "some cuts left an entry");
}

/// MENU-hold's save of a project with three sources writes the same bytes
/// whatever the sources hold: refs, never audio.
#[test]
fn quick_save_writes_no_audio() {
    let len_with = |frames: u32| {
        let mut s = chimera_fat::FatStore::new(with_clusters(CLUSTERS, 0x5EC));
        let (mut p, _) = Project::boxed();
        for seed in 0..3 {
            let (h, data) = source(seed, frames);
            p.source_store(h.sref, &held(&h, &data)).unwrap();
        }
        let mut card = Card::new();
        assert!(matches!(
            save_at(&mut card, &mut s, &mut p, 1),
            ProjectNote::Saved(_)
        ));
        // The quick save: SAVE over its own file.
        assert!(matches!(
            save_at(&mut card, &mut s, &mut p, 1),
            ProjectNote::Saved(_)
        ));
        let vol = s.mount().unwrap();
        let mut sizes = Vec::new();
        s.list(vol, Dir::Projects, &mut |_, n| sizes.push(n))
            .unwrap();
        assert_eq!(
            s.list(vol, Dir::Tapes, &mut |_, _| {}),
            Err(StoreError::NotFound)
        );
        sizes
    };
    assert_eq!(len_with(64), len_with(20_000));
}

/// The same audio under a new root names the same file: Present, one file.
#[test]
fn same_audio_new_root_is_present() {
    let (h, data) = source(8, FRAMES);
    let disk = Rc::new(with_clusters(CLUSTERS, 0x5EC));
    assert_eq!(write_on(&mut probed(&disk), &h, &data), Ok(Wrote::New));
    let moved = SourceHead { root: 7_200, ..h };
    let mut s = probed(&disk);
    assert_eq!(write_on(&mut s, &moved, &data), Ok(Wrote::Present));
    let vol = s.mount().unwrap();
    let mut files = Vec::new();
    s.list(vol, Dir::Tapes, &mut |f, _| files.push(f)).unwrap();
    assert_eq!(files, vec![source_file(h.sref)]);
}
