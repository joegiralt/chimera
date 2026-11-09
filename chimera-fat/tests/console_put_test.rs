//! The console's `put` and `ls` on a FAT card in RAM, through `answer` as
//! the shells drive them: a project file saved the unit's way, every
//! failure leaving the pair loading as before, with no cluster lost.

#[path = "common/console.rs"]
mod console;
#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;
#[path = "common/tools.rs"]
mod tools;
#[path = "common/tree.rs"]
mod tree;

use chimera_core::console::{
    PUT_CHUNK_BYTES, PUT_DEADLINE_MS, PUT_MAX_BYTES, PutArg, PutTo, find_staged, put,
};
use chimera_core::dsp::tape_engine::read::DESC_LEN;
use chimera_core::factory;
use chimera_core::name::{Name, SoundName};
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::test_support::full_dense;
use chimera_core::project::{
    Project, ProjectDecoder, encode_new_project, encode_project, list_projects, project_file,
};
use chimera_core::storage::{
    BuildStamp, Card, FileKind, Generation, Header, LibId, LibraryDecoder, ProjectId, RecordWriter,
    Side, SourceHead, encode_library_sound, library_file, load_ab_in_place, peek_header,
    read_source_into, source_name, write_file,
};
use chimera_fat::FatStore;
use chimera_hal::store::{Dir, FileName, Store, StoreError, VolumeId};
use console::*;
use image::{Cut, CutDisk, RamDisk, fat_check, fat16, with_clusters};
use probe::{Probed, Sink, log, probed, swept_clean};
use std::rc::Rc;

const ID: u32 = 14;
const PATH: &str = "/CHIMERA/PROJECTS/P0000014.A";

fn put_line(path: &str, body: &[u8]) -> String {
    format!("put {path} {} {:08x}\n", body.len(), crc(body))
}

/// A whole project file, NEW's records, named `name`, at generation `g`.
fn project_bytes(name: &str, g: u32) -> Vec<u8> {
    file_of(name, g, &mut |w| encode_new_project(w))
}

/// A project with a full, dense pool: about 60 KB.
fn big_project_bytes(name: &str, g: u32) -> Vec<u8> {
    let (p, _) = full_dense();
    file_of(name, g, &mut |w| encode_project(&p, w))
}

fn file_of(
    name: &str,
    g: u32,
    body: &mut dyn FnMut(&mut RecordWriter<'_>) -> Result<(), StoreError>,
) -> Vec<u8> {
    let h = Header {
        kind: FileKind::Project,
        generation: Generation::new(g),
        name: Some(Name::new(name).unwrap()),
    };
    let mut out = Bytes(Vec::new());
    write_file(&mut out, &h, body).unwrap();
    out.0
}

fn side(s: Side) -> FileName {
    project_file(ProjectId::new(ID).unwrap()).side(s)
}

/// A FAT16 card with `/CHIMERA` and its id.
fn card<S: Store>(s: &mut S) -> VolumeId {
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    vol
}

fn read<S: Store>(s: &mut S, vol: VolumeId, f: FileName) -> Option<Vec<u8>> {
    let mut sink = Sink::default();
    s.read(vol, f, &mut sink).ok().map(|()| sink.0)
}

/// The project a load takes: its name and generation.
fn loads<S: Store>(s: &mut S) -> Option<(String, Generation)> {
    let (mut p, _) = Project::boxed();
    let h = Card::new()
        .run(s, |s, r| {
            load_ab_in_place(
                s,
                r,
                project_file(ProjectId::new(ID).unwrap()),
                &mut ProjectDecoder::new_for_test(&mut p),
            )
        })
        .ok()?
        .result
        .ok()?;
    Some((h.name.unwrap().as_str().to_owned(), h.generation))
}

/// A card holding the pair at generations 5 (A) and 6 (B), named OLD.
fn old_pair<S: Store>(s: &mut S) -> VolumeId {
    let vol = card(s);
    s.make_dir(vol, Dir::Projects).unwrap();
    for (sd, g) in [(Side::A, 5), (Side::B, 6)] {
        let b = project_bytes("OLD", g);
        s.write(vol, side(sd), &mut |w| w.put(&b)).unwrap();
    }
    vol
}

/// Nothing cross-linked, no file over its chain, no cluster lost.
fn clean(disk: &RamDisk) {
    let r = fat_check(disk);
    assert!(
        r.cross_linked.is_empty(),
        "cross-linked {:?}",
        r.cross_linked
    );
    assert!(r.short.is_empty(), "over their chains {:?}", r.short);
    assert!(r.lost.is_empty(), "lost {:?}", r.lost);
}

#[test]
fn a_put_onto_an_empty_card_loads() {
    let mut s = FatStore::new(fat16(16_384, 1));
    let vol = card(&mut s);
    let body = project_bytes("AVRIL 14", 9);
    assert_eq!(
        ask(&mut s, &put_line(PATH, &body), &lines(&body)),
        "READY\nOK\n"
    );
    // The unit's generation, not the file's: the first save.
    assert_eq!(loads(&mut s), Some(("AVRIL 14".into(), Generation::FIRST)));
    let a = read(&mut s, vol, side(Side::A)).unwrap();
    assert_eq!(a.len(), body.len());
    assert_eq!(
        a[28..a.len() - 4],
        body[28..body.len() - 4],
        "the records as sent"
    );
    assert_eq!(read(&mut s, vol, side(Side::B)), None);
    clean(s.device());
    assert_eq!(
        ask(&mut s, "ls\n", b""),
        format!("P0000014.A {}\nOK\n", body.len())
    );
}

/// An older generation in the file is renumbered past the pair: it is what
/// loads, never hidden behind its sibling, and the kept side is untouched.
#[test]
fn a_put_lands_on_the_unkept_side_as_the_newest() {
    let mut s = FatStore::new(fat16(16_384, 1));
    let vol = old_pair(&mut s);
    let kept = read(&mut s, vol, side(Side::B)).unwrap();
    let body = project_bytes("AVRIL 14", 1);
    assert_eq!(
        ask(
            &mut s,
            &put_line("/chimera/projects/p0000014.b", &body),
            &lines(&body)
        ),
        "READY\nOK\n"
    );
    assert_eq!(loads(&mut s), Some(("AVRIL 14".into(), Generation::new(7))));
    assert_eq!(
        read(&mut s, vol, side(Side::B)).unwrap(),
        kept,
        "B untouched"
    );
    let a = read(&mut s, vol, side(Side::A)).unwrap();
    let h = peek_header(a[..28].try_into().unwrap()).unwrap();
    assert_eq!(h.generation, Generation::new(7));
    clean(s.device());
}

/// Each failure after READY: its `ERR`, the old pair loading as it did,
/// the kept side untouched, nothing lost, and the host's data all read.
fn fails_after_ready(line: String, data: &[u8], per_byte_us: u64, err: &str) {
    fails_draining(line, data, per_byte_us, err, true)
}

/// As `fails_after_ready`; a slow host isn't drained within `DRAIN_MS`.
fn fails_draining(line: String, data: &[u8], per_byte_us: u64, err: &str, drained: bool) {
    let mut s = FatStore::new(fat16(16_384, 1));
    let vol = old_pair(&mut s);
    let kept = read(&mut s, vol, side(Side::B)).unwrap();
    let (said, unread) = ask_on(&mut s, &line, data, per_byte_us);
    assert_eq!(said, format!("READY\nERR put: {err}\n"));
    if drained {
        assert_eq!(unread, 0, "drained");
    }
    assert_eq!(loads(&mut s), Some(("OLD".into(), Generation::new(6))));
    assert_eq!(read(&mut s, vol, side(Side::B)).unwrap(), kept);
    clean(s.device());
}

#[test]
fn a_bad_crc_is_refused() {
    let body = project_bytes("AVRIL 14", 1);
    let line = format!("put {PATH} {} {:08x}\n", body.len(), crc(&body) ^ 1);
    fails_after_ready(line, &lines(&body), 0, "CRC mismatch");
}

#[test]
fn a_file_whose_own_crc_fails_is_refused() {
    let mut body = project_bytes("AVRIL 14", 1);
    let n = body.len();
    body[n - 1] ^= 1;
    fails_after_ready(
        put_line(PATH, &body),
        &lines(&body),
        0,
        "FILE CHECKSUM FAILED",
    );
}

#[test]
fn a_file_the_project_check_refuses_never_shadows_the_old() {
    // An unknown must-understand record: a load would refuse the side as
    // needing newer firmware. It never gets its trailer.
    let body = project_bytes("AVRIL 14", 1);
    let mut odd = body[..body.len() - 4].to_vec();
    odd.extend_from_slice(&[0x77, 0xF7, 0, 0]);
    let c = crc(&odd).to_le_bytes();
    odd.extend_from_slice(&c);
    fails_after_ready(
        put_line(PATH, &odd),
        &lines(&odd),
        0,
        "NEEDS NEWER FIRMWARE",
    );
}

#[test]
fn a_short_transfer_stalls() {
    let body = project_bytes("AVRIL 14", 1);
    let half = lines(&body[..body.len() / 2 / 48 * 48]);
    fails_after_ready(put_line(PATH, &body), &half, 0, "no block for 500 ms");
}

#[test]
fn a_host_under_a_block_in_500_ms_stalls() {
    // 1 ms a byte: a block's 690 line bytes take 690 ms.
    let body = project_bytes("AVRIL 14", 1);
    fails_draining(
        put_line(PATH, &body),
        &lines(&body),
        1_000,
        "no block for 500 ms",
        false,
    );
}

#[test]
fn an_abort_mid_way_stops_it() {
    let body = project_bytes("AVRIL 14", 1);
    let mut data = lines(&body[..2_400]);
    data.extend_from_slice(b"abort\n");
    data.extend_from_slice(&lines(&body[2_400..]));
    fails_after_ready(put_line(PATH, &body), &data, 0, "aborted");
}

#[test]
fn more_data_than_the_size_is_refused() {
    // Cut the fewest bytes (at least one) that leave the last line carrying
    // more than one, whatever the project's length.
    let full = project_bytes("AVRIL 14", 1);
    let cut = (1..).find(|c| (full.len() - c) % 48 > 1).unwrap();
    let body = &full[..full.len() - cut];
    assert!(
        body.len() % 48 > 1,
        "the last line carries more than one byte"
    );
    // Declared a byte short of what the last line carries.
    let line = format!("put {PATH} {} 0\n", body.len() - 1);
    fails_after_ready(line, &lines(body), 0, "more data than its size");
}

#[test]
fn bad_lines_are_refused() {
    let body = project_bytes("AVRIL 14", 1);
    let start = lines(&body[..96]);
    for bad in [
        &b"not base64!\n"[..],
        b"QUJD=QUJD\n",
        b"QUJ\n",
        &[b'A'; 68],
        b"QQ==QUJD\n",
        // Padding that ends a line but not the file.
        b"QQ==\n",
    ] {
        let data = [&start[..], bad].concat();
        fails_after_ready(put_line(PATH, &body), &data, 0, "bad data line");
    }
}

#[test]
fn a_put_past_the_deadline_times_out() {
    // 0.6 ms a byte: each block in time, the whole file past 30 s.
    let body = big_project_bytes("AVRIL 14", 1);
    assert!(
        lines(&body).len() as u64 * 600 > u64::from(PUT_DEADLINE_MS) * 1_000,
        "{} B",
        body.len()
    );
    fails_draining(put_line(PATH, &body), &lines(&body), 600, "timeout", false);
}

/// Refusals before READY: nothing read, nothing written.
#[test]
fn refusals_before_ready_write_nothing() {
    let mut s = FatStore::new(fat16(16_384, 1));
    old_pair(&mut s);
    let before = s.device().0.borrow().clone();
    for line in [
        format!("put {PATH} {} 0\n", PUT_MAX_BYTES + 1),
        format!("put {PATH} 31 0\n"),
    ] {
        assert!(ask(&mut s, &line, b"").starts_with("ERR put: "), "{line}");
    }
    let usage = "ERR put takes /CHIMERA/PROJECTS/P#######.A, /CHIMERA/SOUNDS/S#######.A [--over], \
                 /CHIMERA/TAPES/<CRC>.SRC, /CHIMERA/BENCH/BEDS.CHK, /CHIMERA/TUNINGS/TUNE#.SCL|KBM, \
                 /CHIMERA/MAPS/<stem>.A, \
                 or /CHIMERA/SYSTEM/BANK2.BK2, its size and CRC-32 in hex\n";
    for path in [
        "/CHIMERA/PROJECTS/AVRIL14.A",
        "/CHIMERA/PROJECTS/../SYSTEM.A",
        "/CHIMERA/PROJECTS//P0000014.A",
        "/CHIMERA/PROJECTS/",
        "/CHIMERA/SOUNDS/S0000001",
        "/CHIMERA/SOUNDS/../SYSTEM.A",
        "/CHIMERA/SOUNDS//S0000001.A",
        "/CHIMERA/SYSTEM.A",
        "/CHIMERA/BENCH/CARD0.BIN",
        "/CHIMERA/SYSTEM/BANK2.BIN",
        "/CHIMERA/SYSTEM/BANK2.BK",
        "PROJECTS/P0000014.A",
    ] {
        assert_eq!(
            ask(&mut s, &format!("put {path} 1 0\n"), b"QQ==\n"),
            usage,
            "{path}"
        );
    }
    assert!(
        s.device().0.borrow().clone() == before,
        "not a block written"
    );
}

/// A refused put to an id the card doesn't hold leaves no project to list:
/// the side it wrote goes with it.
#[test]
fn a_refused_put_to_a_new_id_lists_nothing() {
    let mut s = FatStore::new(fat16(16_384, 1));
    card(&mut s);
    let body = big_project_bytes("AVRIL 14", 1);
    let line = format!("put {PATH} {} {:08x}\n", body.len(), crc(&body) ^ 1);
    let (said, _) = ask_on(&mut s, &line, &lines(&body), 0);
    assert_eq!(said, "READY\nERR put: CRC mismatch\n");
    let mut listed = Vec::new();
    let _ = list_projects(&mut Card::new(), &mut s, &mut |e| listed.push(e.id));
    assert_eq!(listed, []);
    assert_eq!(ask(&mut s, "ls\n", b""), "OK\n");
    clean(s.device());
}

#[test]
fn the_open_project_is_refused() {
    let mut s = FatStore::new(fat16(16_384, 1));
    old_pair(&mut s);
    let body = project_bytes("AVRIL 14", 1);
    let arg = PutArg {
        to: PutTo::Project(ProjectId::new(ID).unwrap()),
        size: body.len() as u32,
        crc: crc(&body),
        resume: false,
    };
    let mut host = Host::new(&lines(&body), 0);
    put(arg, ProjectId::new(ID), None, &mut s, &mut host).unwrap();
    assert_eq!(
        String::from_utf8(host.out).unwrap(),
        "ERR put: project is open\n"
    );
    assert_eq!(loads(&mut s), Some(("OLD".into(), Generation::new(6))));
}

#[test]
fn a_sound_or_a_wrong_header_is_refused_before_a_write() {
    let mut s = FatStore::new(fat16(16_384, 1));
    old_pair(&mut s);
    let mut body = project_bytes("AVRIL 14", 1);
    body[6] = FileKind::Sound as u8;
    let (said, _) = ask_on(&mut s, &put_line(PATH, &body), &lines(&body), 0);
    assert_eq!(said, "READY\nERR put: WRONG FILE TYPE\n");
    body[6] = FileKind::Project as u8;
    body[4] = 2;
    let (said, _) = ask_on(&mut s, &put_line(PATH, &body), &lines(&body), 0);
    assert_eq!(said, "READY\nERR put: NEEDS NEWER FIRMWARE\n");
    assert_eq!(loads(&mut s), Some(("OLD".into(), Generation::new(6))));
    clean(s.device());
}

#[test]
fn a_read_only_side_is_refused() {
    let mut s = FatStore::new(fat16(16_384, 1));
    old_pair(&mut s);
    // A is the side a save writes: mark it read-only.
    let r = fat_check(s.device());
    let a = r.entry(b"P0000014A  ").unwrap().clone();
    let at = s
        .device()
        .block(a.block)
        .chunks(32)
        .position(|c| c == a.raw)
        .unwrap();
    s.device().0.borrow_mut()[a.block as usize][at * 32 + 11] |= 0x01;
    let body = project_bytes("AVRIL 14", 1);
    let (said, _) = ask_on(&mut s, &put_line(PATH, &body), &lines(&body), 0);
    assert_eq!(said, "READY\nERR put: FILE IS READ-ONLY\n");
    assert_eq!(loads(&mut s), Some(("OLD".into(), Generation::new(6))));
}

#[test]
fn a_full_card_fails_with_the_old_pair_whole() {
    // FAT16's fewest clusters, the old pair on it, then filled but 20 KB.
    let mut s = FatStore::new(with_clusters(4_085, 1));
    let vol = old_pair(&mut s);
    let free = s.free_bytes(vol).unwrap() as usize;
    let filler = FileName::new(Dir::Chimera, b"FILL", b"BIN").unwrap();
    s.write(vol, filler, &mut |w| w.put(&vec![0; free - 20_000]))
        .unwrap();
    let body = big_project_bytes("AVRIL 14", 1);
    assert!(body.len() > 20_000);
    let (said, unread) = ask_on(&mut s, &put_line(PATH, &body), &lines(&body), 0);
    assert_eq!(said, "READY\nERR put: CARD FULL\n");
    assert_eq!(unread, 0);
    assert_eq!(loads(&mut s), Some(("OLD".into(), Generation::new(6))));
    clean(s.device());
}

#[test]
fn ls_lists_projects_and_their_sizes() {
    let mut s = FatStore::new(fat16(16_384, 1));
    let vol = card(&mut s);
    assert_eq!(ask(&mut s, "ls\n", b""), "ERR ls: FILE NOT FOUND\n");
    s.make_dir(vol, Dir::Projects).unwrap();
    for (stem, n) in [(&b"P0000001"[..], 10), (b"P0000002", 2_000)] {
        let f = FileName::new(Dir::Projects, stem, b"A").unwrap();
        s.write(vol, f, &mut |w| w.put(&vec![0; n])).unwrap();
    }
    let f = FileName::new(Dir::Projects, b"README", b"").unwrap();
    s.write(vol, f, &mut |w| w.put(b"hi")).unwrap();
    assert_eq!(
        ask(&mut s, "ls\n", b""),
        "P0000001.A 10\nP0000002.A 2000\nREADME 2\nOK\n"
    );
    assert!(ask(&mut s, "ls /CHIMERA\n", b"").starts_with("ERR ls takes"));
}

// Device errors and power cuts, at every block a put writes.

/// A card holding the old pair, as a `CutDisk` slot.
fn cut_base() -> RamDisk {
    let mut s = FatStore::new(with_clusters(4_200, 0xC0DE));
    old_pair(&mut s);
    let RamDisk(blocks) = s.device();
    RamDisk(blocks.clone())
}

fn copy(d: &RamDisk) -> RamDisk {
    RamDisk(d.0.clone())
}

/// The blocks a put of `body` writes on `base`, in order.
fn put_writes(base: &RamDisk, body: &[u8]) -> Vec<u32> {
    let disk = Rc::new(copy(base));
    let mut s: Probed<RamDisk> = probed(&disk);
    assert_eq!(
        ask(&mut s, &put_line(PATH, body), &lines(body)),
        "READY\nOK\n"
    );
    log(&s).writes.borrow().iter().map(|&(b, _)| b).collect()
}

/// A device error at each block a put writes, the card still powered: the
/// put fails, the old pair loads, and the chains it left unheld are freed,
/// at once or by the next change when the card refused the FAT write.
#[test]
fn a_device_error_at_any_write_loses_nothing() {
    let base = cut_base();
    let body = big_project_bytes("AVRIL 14", 1);
    let blocks = put_writes(&base, &body);
    assert!(blocks.len() > 20);
    // The directory block holding A's entry: an entry write that fails
    // keeps its chain (#360), the one leak allowed.
    let entry_block = fat_check(&base).entry(b"P0000014A  ").unwrap().block;
    for (k, &block) in blocks.iter().enumerate() {
        let what = format!("write {k} of {} (block {block})", blocks.len());
        let disk = Rc::new(copy(&base));
        let mut s: Probed<RamDisk> = probed(&disk);
        log(&s).fail_write.set(Some(block));
        let (said, unread) = ask_on(&mut s, &put_line(PATH, &body), &lines(&body), 0);
        assert!(said.starts_with("READY\nERR put: "), "{what}: {said}");
        assert_eq!(unread, 0, "{what}: drained");
        log(&s).fail_write.set(None);
        // A chain the card refused to free, and a FAT 2 sector it missed,
        // wait for the next change, as any save would make.
        let vol = s.mount().unwrap();
        let next = FileName::new(Dir::Chimera, b"NEXT", b"BIN").unwrap();
        s.write(vol, next, &mut |w| w.put(b"x")).unwrap();
        let r = fat_check(&disk);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty(),
            "{what}: {r:?}"
        );
        assert!(r.fats_differ.is_empty(), "{what}: {r:?}");
        if block == entry_block {
            assert!(
                r.lost.len() <= body.len().div_ceil(512),
                "{what}: lost {:?}",
                r.lost
            );
        } else {
            assert!(r.lost.is_empty(), "{what}: lost {:?}", r.lost);
        }
        assert_eq!(
            loads(&mut s),
            Some(("OLD".into(), Generation::new(6))),
            "{what}"
        );
    }
}

/// A card that takes a block then errs, at each block a put writes: the
/// put may fail, but whatever the card holds, no two files ever share a
/// cluster, through the next writes and a put that lands.
#[test]
fn a_write_that_lands_then_errs_never_cross_links() {
    let base = cut_base();
    let body = big_project_bytes("AVRIL 14", 1);
    let blocks = put_writes(&base, &body);
    for (k, &block) in blocks.iter().enumerate() {
        let what = format!("write {k} of {} (block {block})", blocks.len());
        let disk = Rc::new(copy(&base));
        let mut s: Probed<RamDisk> = probed(&disk);
        log(&s).land_then_fail.set(Some(k));
        let (said, _) = ask_on(&mut s, &put_line(PATH, &body), &lines(&body), 0);
        assert!(said.starts_with("READY\n"), "{what}: {said}");
        let r = fat_check(&disk);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty(),
            "{what}: {r:?}"
        );
        // After a power cycle: the allocator starts from the FAT's front.
        let mut s: Probed<RamDisk> = probed(&disk);
        let vol = s.mount().unwrap();
        let next = FileName::new(Dir::Chimera, b"NEXT", b"BIN").unwrap();
        s.write(vol, next, &mut |w| w.put(&[7; 30_000])).unwrap();
        let again = big_project_bytes("AGAIN", 1);
        assert_eq!(
            ask(&mut s, &put_line(PATH, &again), &lines(&again)),
            "READY\nOK\n",
            "{what}"
        );
        let r = fat_check(&disk);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty(),
            "{what}: {r:?}"
        );
        assert_eq!(read(&mut s, vol, next).unwrap(), [7; 30_000], "{what}");
        let got = loads(&mut s).map(|(n, _)| n);
        assert_eq!(got.as_deref(), Some("AGAIN"), "{what}");
    }
}

/// Power cut after each block a put writes: nothing cross-linked or over
/// its chain, and the old pair loads, or the new file whole. A cut mid-
/// write can leave the new side's clusters lost (ADR 0048: no entry holds
/// them yet); they are counted, never the kept side's.
#[test]
fn a_power_cut_at_any_write_keeps_a_generation() {
    let base = cut_base();
    let body = big_project_bytes("AVRIL 14", 1);
    let blocks = put_writes(&base, &body);
    let mut most_lost = 0;
    for k in 0..blocks.len() {
        let what = format!("cut after write {k} of {}", blocks.len());
        let slot = Rc::new(CutDisk::new(copy(&base), Cut::After(k as u32)));
        let mut s: Probed<CutDisk> = probed(&slot);
        let (said, _) = ask_on(&mut s, &put_line(PATH, &body), &lines(&body), 0);
        assert!(said.starts_with("READY\n"), "{what}: {said}");
        slot.cut.set(Cut::Never);
        let r = fat_check(&slot.inner);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty(),
            "{what}: {r:?}"
        );
        most_lost = most_lost.max(r.lost.len());
        let got = loads(&mut probed(&slot));
        assert!(
            got == Some(("OLD".into(), Generation::new(6)))
                || got == Some(("AVRIL 14".into(), Generation::new(7))),
            "{what}: {got:?}"
        );
        swept_clean(&what, &slot, &slot.inner);
        assert_eq!(loads(&mut probed(&slot)), got, "{what}: swept");
    }
    let new_clusters = body.len().div_ceil(512);
    assert!(most_lost <= new_clusters, "{most_lost} lost");
    println!("power cut: at most {most_lost} clusters lost of {new_clusters}");
}

#[test]
fn a_device_error_mid_write_frees_the_chain() {
    // The FAT layer alone: a write that dies mid-stream on a data block.
    let disk = Rc::new(fat16(16_384, 1));
    let mut s: Probed<RamDisk> = probed(&disk);
    let vol = card(&mut s);
    let f = FileName::new(Dir::Chimera, b"DATA", b"BIN").unwrap();
    s.write(vol, f, &mut |w| w.put(&[1; 3_000])).unwrap();
    let blocks: Vec<u32> = {
        let probe = Rc::new(copy(&disk));
        let mut t: Probed<RamDisk> = probed(&probe);
        t.write(vol, f, &mut |w| w.put(&[2; 20_000])).unwrap();
        log(&t).writes.borrow().iter().map(|&(b, _)| b).collect()
    };
    for &block in &blocks {
        let d = Rc::new(copy(&disk));
        let mut t: Probed<RamDisk> = probed(&d);
        log(&t).fail_write.set(Some(block));
        assert!(t.write(vol, f, &mut |w| w.put(&[2; 20_000])).is_err());
        log(&t).fail_write.set(None);
        // What a failed flush leaves for later is freed by the next change.
        let other = FileName::new(Dir::Chimera, b"NEXT", b"BIN").unwrap();
        t.write(vol, other, &mut |w| w.put(b"x")).unwrap();
        let r = fat_check(&d);
        assert!(r.lost.is_empty(), "block {block}: lost {:?}", r.lost);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty(),
            "block {block}"
        );
    }
}

/// dosfstools' `fsck.fat -n` on what puts leave: a good one, then a device
/// error at every block one writes, each followed by the next change.
#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn fsck_passes_after_puts_and_device_errors() {
    use chimera_fat::volume::FsKind;
    let mut s = FatStore::new(tools::mkfs(FsKind::Fat16, 30_000, 1, 0x1600_0099));
    old_pair(&mut s);
    let base = copy(s.device());
    let body = big_project_bytes("AVRIL 14", 1);
    let f = tools::fsck(&base);
    assert_eq!(f.code, 0, "the old pair: {}", f.out);
    let blocks = put_writes(&base, &body);
    for (k, &block) in blocks.iter().enumerate().step_by(7) {
        let disk = Rc::new(copy(&base));
        let mut s: Probed<RamDisk> = probed(&disk);
        log(&s).fail_write.set(Some(block));
        let (said, _) = ask_on(&mut s, &put_line(PATH, &body), &lines(&body), 0);
        assert!(said.starts_with("READY\nERR put: "), "write {k}: {said}");
        log(&s).fail_write.set(None);
        let vol = s.mount().unwrap();
        let next = FileName::new(Dir::Chimera, b"NEXT", b"BIN").unwrap();
        s.write(vol, next, &mut |w| w.put(b"x")).unwrap();
        let f = tools::fsck(&disk);
        assert_eq!(f.code, 0, "write {k} (block {block}): {}", f.out);
    }
    let disk = Rc::new(copy(&base));
    let mut s: Probed<RamDisk> = probed(&disk);
    assert_eq!(
        ask(&mut s, &put_line(PATH, &body), &lines(&body)),
        "READY\nOK\n"
    );
    let f = tools::fsck(&disk);
    assert_eq!(f.code, 0, "after a good put: {}", f.out);
}

/// The FAT layer alone, a write whose block lands then errs at each block
/// it writes: whatever the entry ends up holding, the next write never
/// shares a cluster with it.
#[test]
fn a_write_that_lands_then_errs_leaves_no_shared_cluster() {
    let disk = Rc::new(fat16(16_384, 1));
    let mut s: Probed<RamDisk> = probed(&disk);
    let vol = card(&mut s);
    let f = FileName::new(Dir::Chimera, b"DATA", b"BIN").unwrap();
    s.write(vol, f, &mut |w| w.put(&[1; 3_000])).unwrap();
    let blocks: Vec<u32> = {
        let probe = Rc::new(copy(&disk));
        let mut t: Probed<RamDisk> = probed(&probe);
        t.write(vol, f, &mut |w| w.put(&[2; 20_000])).unwrap();
        log(&t).writes.borrow().iter().map(|&(b, _)| b).collect()
    };
    for (k, &block) in blocks.iter().enumerate() {
        let d = Rc::new(copy(&disk));
        let mut t: Probed<RamDisk> = probed(&d);
        log(&t).land_then_fail.set(Some(k));
        let _ = t.write(vol, f, &mut |w| w.put(&[2; 20_000]));
        // After a power cycle: the allocator starts from the FAT's front.
        let mut t: Probed<RamDisk> = probed(&d);
        let other = FileName::new(Dir::Chimera, b"NEXT", b"BIN").unwrap();
        t.write(vol, other, &mut |w| w.put(&[3; 20_000])).unwrap();
        let r = fat_check(&d);
        assert!(
            r.cross_linked.is_empty(),
            "block {block}: {:?}",
            r.cross_linked
        );
        assert!(r.short.is_empty(), "block {block}: {:?}", r.short);
    }
}

// Sources (ADR 0106): chunked, written once, sealed, under the same faults.

/// Two chunks: about 270 KB.
fn two_chunks() -> (SourceHead, Vec<u8>) {
    let (h, f) = source_file_bytes(5, 180_000);
    assert!(f.len() > PUT_CHUNK_BYTES as usize && f.len() < 2 * PUT_CHUNK_BYTES as usize);
    (h, f)
}

const SRC_OK: &str = "READY\nACK 262144\nACK 272214\nOK\n";

/// Whether the source reads whole, as a Part's SOURCE reads it.
fn source_loads<S: Store>(s: &mut S, h: &SourceHead) -> bool {
    let mut out = vec![0; DESC_LEN + 3 * h.sref.frames.div_ceil(2) as usize];
    Card::new()
        .run(s, |s, r| read_source_into(s, r, h.sref, &mut out))
        .and_then(|o| o.result)
        .is_ok()
}

/// The blocks a source put writes on `base`, in order, and the blocks of
/// the entries it writes: TAPES's in CHIMERA, and the file's.
fn source_put_writes(base: &RamDisk, h: &SourceHead, file: &[u8]) -> (Vec<u32>, [u32; 2]) {
    let disk = Rc::new(copy(base));
    let mut s: Probed<RamDisk> = probed(&disk);
    assert_eq!(ask(&mut s, &src_line(h, file), &chunked(file)), SRC_OK);
    let r = fat_check(&disk);
    let stem = format!("{:08X}SRC", h.sref.crc);
    let entries = [&b"TAPES      "[..], stem.as_bytes()]
        .map(|n| r.entry(n.try_into().unwrap()).unwrap().block);
    let writes = log(&s).writes.borrow().iter().map(|&(b, _)| b).collect();
    (writes, entries)
}

#[test]
fn a_source_put_lands_sealed_and_lists() {
    let mut s = FatStore::new(cut_base());
    let (h, file) = two_chunks();
    assert_eq!(file.len(), 272_214);
    assert_eq!(ask(&mut s, &src_line(&h, &file), &chunked(&file)), SRC_OK);
    assert!(source_loads(&mut s, &h));
    let vol = s.mount().unwrap();
    assert_eq!(
        s.delete(vol, source_name(h.sref.stem())),
        Err(StoreError::ReadOnly)
    );
    assert_eq!(
        ask(&mut s, "ls tapes\n", b""),
        format!("{:08X}.SRC {}\nOK\n", h.sref.crc, file.len())
    );
    assert_eq!(
        ask(&mut s, &src_line(&h, &file), &chunked(&file)),
        "OK PRESENT\n"
    );
    assert_eq!(loads(&mut s), Some(("OLD".into(), Generation::new(6))));
    clean(s.device());
}

/// A device error at each block a source put writes: the put fails, no
/// file is left, nothing is cross-linked or short, and nothing is lost but
/// for the entry's own write (#360); the old pair still loads.
#[test]
fn a_device_error_at_any_source_write_loses_nothing() {
    let base = cut_base();
    let (h, file) = two_chunks();
    let (blocks, entries) = source_put_writes(&base, &h, &file);
    assert!(blocks.len() > 500, "{}", blocks.len());
    for (k, &block) in blocks.iter().enumerate() {
        let what = format!("write {k} of {} (block {block})", blocks.len());
        let disk = Rc::new(copy(&base));
        let mut s: Probed<RamDisk> = probed(&disk);
        log(&s).fail_write.set(Some(block));
        let (said, unread) = ask_on(&mut s, &src_line(&h, &file), &chunked(&file), 0);
        log(&s).fail_write.set(None);
        assert!(said.contains("ERR put: "), "{what}: {said}");
        // Before READY (making TAPES) the host has sent nothing to drain.
        if said.starts_with("READY") {
            assert_eq!(unread, 0, "{what}: drained");
        }
        let vol = s.mount().unwrap();
        let next = FileName::new(Dir::Chimera, b"NEXT", b"BIN").unwrap();
        s.write(vol, next, &mut |w| w.put(b"x")).unwrap();
        let r = fat_check(&disk);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty() && r.fats_differ.is_empty(),
            "{what}: {r:?}"
        );
        // An entry write that fails keeps the chain it names (#360): the
        // TAPES directory's cluster, or the file's.
        if entries.contains(&block) {
            assert!(r.lost.len() <= file.len().div_ceil(512), "{what}: {r:?}");
        } else {
            assert!(r.lost.is_empty(), "{what}: lost {:?}", r.lost);
        }
        assert!(
            !source_loads(&mut s, &h),
            "{what}: a failed put left a source"
        );
        assert_eq!(
            loads(&mut s),
            Some(("OLD".into(), Generation::new(6))),
            "{what}"
        );
    }
}

/// A card that takes a block then errs, at each block a source put
/// writes: no two files share a cluster, and after a power cycle the
/// re-run lands the source (or finds it there) beside the next writes.
#[test]
fn a_source_write_that_lands_then_errs_never_cross_links() {
    let base = cut_base();
    let (h, file) = two_chunks();
    let (blocks, _) = source_put_writes(&base, &h, &file);
    for k in 0..blocks.len() {
        let what = format!("write {k} of {}", blocks.len());
        let disk = Rc::new(copy(&base));
        let mut s: Probed<RamDisk> = probed(&disk);
        log(&s).land_then_fail.set(Some(k));
        let _ = ask_on(&mut s, &src_line(&h, &file), &chunked(&file), 0);
        let r = fat_check(&disk);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty(),
            "{what}: {r:?}"
        );
        let mut s: Probed<RamDisk> = probed(&disk);
        let vol = s.mount().unwrap();
        let next = FileName::new(Dir::Chimera, b"NEXT", b"BIN").unwrap();
        s.write(vol, next, &mut |w| w.put(&[7; 30_000])).unwrap();
        let again = ask(&mut s, &src_line(&h, &file), &chunked(&file));
        assert!(
            again == SRC_OK || again == "OK PRESENT\n",
            "{what}: {again}"
        );
        let r = fat_check(&disk);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty(),
            "{what}: {r:?}"
        );
        assert_eq!(read(&mut s, vol, next).unwrap(), [7; 30_000], "{what}");
        assert!(source_loads(&mut s, &h), "{what}");
    }
}

/// Power cut after each block a source put writes: nothing cross-linked
/// or short; the source reads whole or not at all, never shorter; the
/// sweep frees what the cut lost; the re-run lands it once.
#[test]
fn a_power_cut_at_any_source_write_reads_whole_or_missing() {
    let base = cut_base();
    let (h, file) = two_chunks();
    let (blocks, _) = source_put_writes(&base, &h, &file);
    let mut most_lost = 0;
    for k in 0..blocks.len() {
        let what = format!("cut after write {k} of {}", blocks.len());
        let slot = Rc::new(CutDisk::new(copy(&base), Cut::After(k as u32)));
        let mut s: Probed<CutDisk> = probed(&slot);
        let _ = ask_on(&mut s, &src_line(&h, &file), &chunked(&file), 0);
        slot.cut.set(Cut::Never);
        let r = fat_check(&slot.inner);
        assert!(
            r.cross_linked.is_empty() && r.short.is_empty(),
            "{what}: {r:?}"
        );
        most_lost = most_lost.max(r.lost.len());
        swept_clean(&what, &slot, &slot.inner);
        let mut s: Probed<CutDisk> = probed(&slot);
        let again = ask(&mut s, &src_line(&h, &file), &chunked(&file));
        assert!(
            again == SRC_OK || again == "OK PRESENT\n",
            "{what}: {again}"
        );
        assert!(source_loads(&mut s, &h), "{what}");
        assert_eq!(
            loads(&mut s),
            Some(("OLD".into(), Generation::new(6))),
            "{what}"
        );
    }
    let new_clusters = file.len().div_ceil(512);
    assert!(most_lost <= new_clusters, "{most_lost} lost");
    println!("source power cut: at most {most_lost} clusters lost of {new_clusters}");
}

/// dosfstools' `fsck.fat -n` after a source put, and after a device error
/// at every 13th block one writes, each followed by the next change.
#[test]
#[ignore = "needs dosfstools: just test-fat-tools"]
fn fsck_passes_after_source_puts_and_device_errors() {
    use chimera_fat::volume::FsKind;
    let mut s = FatStore::new(tools::mkfs(FsKind::Fat16, 30_000, 1, 0x1600_0106));
    old_pair(&mut s);
    let base = copy(s.device());
    let (h, file) = two_chunks();
    let (blocks, _) = source_put_writes(&base, &h, &file);
    for (k, &block) in blocks.iter().enumerate().step_by(13) {
        let disk = Rc::new(copy(&base));
        let mut s: Probed<RamDisk> = probed(&disk);
        log(&s).fail_write.set(Some(block));
        let _ = ask_on(&mut s, &src_line(&h, &file), &chunked(&file), 0);
        log(&s).fail_write.set(None);
        let vol = s.mount().unwrap();
        let next = FileName::new(Dir::Chimera, b"NEXT", b"BIN").unwrap();
        s.write(vol, next, &mut |w| w.put(b"x")).unwrap();
        let f = tools::fsck(&disk);
        assert_eq!(f.code, 0, "write {k} (block {block}): {}", f.out);
    }
    let disk = Rc::new(copy(&base));
    let mut s: Probed<RamDisk> = probed(&disk);
    assert_eq!(ask(&mut s, &src_line(&h, &file), &chunked(&file)), SRC_OK);
    let f = tools::fsck(&disk);
    assert_eq!(f.code, 0, "after a good source put: {}", f.out);
}

// Library Sounds: `ls sounds`, `put`, `rm` and `get` in `/CHIMERA/SOUNDS`.

const STAMP: BuildStamp = BuildStamp::new("0.1.0", "02e440e2", false);

/// Factory Sound `i`, named as its library file's header names it.
fn sound_of(i: usize) -> Sound {
    let mut s = factory::factory_sound(i).unwrap();
    s.name = SoundName::new("LIB").unwrap();
    s
}

/// Sound `i` as a library file at generation `g`.
fn sound_bytes(i: usize, g: u32) -> Vec<u8> {
    let s = sound_of(i);
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::new(g),
        name: Some(Name::new("LIB").unwrap()),
    };
    let mut out = Bytes(Vec::new());
    write_file(&mut out, &h, &mut |w| {
        encode_library_sound(&s, &STAMP, None, w)
    })
    .unwrap();
    out.0
}

fn sound_name(stem: &str) -> FileName {
    let (stem, ext) = stem.split_once('.').unwrap_or((stem, ""));
    FileName::new(Dir::Sounds, stem.as_bytes(), ext.as_bytes()).unwrap()
}

/// A card with S0000001 (Sound 0, A at 1), S0000002 (Sound 1, A at 1 and
/// B at 2) and the factory S9000001 (Sound 2).
fn library<S: Store>(s: &mut S) -> VolumeId {
    let vol = card(s);
    s.make_dir(vol, Dir::Sounds).unwrap();
    for (name, i, g) in [
        ("S0000001.A", 0, 1),
        ("S0000002.A", 1, 1),
        ("S0000002.B", 1, 2),
        ("S9000001.A", 2, 1),
    ] {
        let b = sound_bytes(i, g);
        s.write(vol, sound_name(name), &mut |w| w.put(&b)).unwrap();
    }
    vol
}

fn on_card<S: Store>(s: &mut S, name: &str) -> Option<Vec<u8>> {
    let vol = s.mount().unwrap();
    read(s, vol, sound_name(name))
}

fn overwrite<S: Store>(s: &mut S, name: &str, b: &[u8]) {
    let vol = s.mount().unwrap();
    s.write(vol, sound_name(name), &mut |w| w.put(b)).unwrap();
}

/// The Sound a library load of `n` takes.
fn load_sound<S: Store>(s: &mut S, n: u32) -> Option<Sound> {
    let mut got = Sound::init(EngineType::Algo);
    let mut kept = chimera_core::midi_map::KeptMap::EMPTY;
    Card::new()
        .run(s, |s, r| {
            load_ab_in_place(
                s,
                r,
                library_file(LibId::new(n).unwrap()),
                &mut LibraryDecoder::new(&mut got, &mut kept),
            )
        })
        .ok()?
        .result
        .ok()?;
    Some(got)
}

fn sound_put(path: &str, body: &[u8], over: bool) -> String {
    let line = put_line(path, body);
    match over {
        true => line.replace('\n', " --over\n"),
        false => line,
    }
}

#[test]
fn ls_sounds_lists_the_library() {
    let mut s = FatStore::new(fat16(16_384, 1));
    let vol = card(&mut s);
    assert_eq!(ask(&mut s, "ls sounds\n", b""), "ERR ls: FILE NOT FOUND\n");
    library(&mut s);
    let f = FileName::new(Dir::Projects, b"P0000001", b"A").unwrap();
    s.make_dir(vol, Dir::Projects).unwrap();
    s.write(vol, f, &mut |w| w.put(b"project")).unwrap();
    let len = |i, g| sound_bytes(i, g).len();
    assert_eq!(
        ask(&mut s, "LS Sounds\n", b""),
        format!(
            "S0000001.A {}\nS0000002.A {}\nS0000002.B {}\nS9000001.A {}\nOK\n",
            len(0, 1),
            len(1, 1),
            len(1, 2),
            len(2, 1)
        )
    );
}

#[test]
fn a_sound_put_onto_a_free_id_loads_and_forgets_the_page() {
    let mut s = FatStore::new(fat16(16_384, 1));
    card(&mut s);
    let body = sound_bytes(3, 7);
    let mut unit = Fake::new(&mut s);
    let (said, _) = ask_unit(
        &mut unit,
        &sound_put("/chimera/sounds/s0000005.b", &body, false),
        &lines(&body),
        0,
    );
    assert_eq!(said, "READY\nOK\n");
    assert_eq!(unit.forgotten, 1);
    let a = on_card(&mut s, "S0000005.A").unwrap();
    let h = peek_header(a[..28].try_into().unwrap()).unwrap();
    assert_eq!(h.generation, Generation::FIRST, "the unit's generation");
    assert_eq!(on_card(&mut s, "S0000005.B"), None);
    assert!(load_sound(&mut s, 5).unwrap().bits_eq(&sound_of(3)));
    clean(s.device());
}

#[test]
fn put_onto_an_existing_id_refuses_unless_over() {
    let mut s = FatStore::new(fat16(16_384, 1));
    library(&mut s);
    let before = on_card(&mut s, "S0000001.A").unwrap();
    let body = sound_bytes(3, 1);
    for path in ["/CHIMERA/SOUNDS/S0000001.A", "/CHIMERA/SOUNDS/S0000001.B"] {
        let (said, unread) = ask_on(&mut s, &sound_put(path, &body, false), &lines(&body), 0);
        assert_eq!(said, "ERR put: exists\n");
        assert_eq!(unread, lines(&body).len(), "no data taken");
    }
    assert_eq!(on_card(&mut s, "S0000001.A").unwrap(), before);
    assert_eq!(on_card(&mut s, "S0000001.B"), None);
    // A torn pair is a pair: refused too.
    overwrite(&mut s, "S0000002.A", b"");
    overwrite(&mut s, "S0000002.B", b"");
    let said = ask(
        &mut s,
        &sound_put("/CHIMERA/SOUNDS/S0000002.A", &body, false),
        &lines(&body),
    );
    assert_eq!(said, "ERR put: exists\n");
    // The factory id is taken by the same rule.
    let said = ask(
        &mut s,
        &sound_put("/CHIMERA/SOUNDS/S9000001.A", &body, false),
        &lines(&body),
    );
    assert_eq!(said, "ERR put: exists\n");
    clean(s.device());
}

#[test]
fn put_over_replaces_the_unit_way() {
    let mut s = FatStore::new(fat16(16_384, 1));
    library(&mut s);
    let kept = on_card(&mut s, "S0000001.A").unwrap();
    let body = sound_bytes(3, 1);
    let said = ask(
        &mut s,
        &sound_put("/CHIMERA/SOUNDS/S0000001.A", &body, true),
        &lines(&body),
    );
    assert_eq!(said, "READY\nOK\n");
    assert_eq!(on_card(&mut s, "S0000001.A").unwrap(), kept, "A kept");
    let b = on_card(&mut s, "S0000001.B").unwrap();
    let h = peek_header(b[..28].try_into().unwrap()).unwrap();
    assert_eq!(h.generation, Generation::new(2));
    assert!(load_sound(&mut s, 1).unwrap().bits_eq(&sound_of(3)));
    // `--over` onto a free id is a plain put.
    let said = ask(
        &mut s,
        &sound_put("/CHIMERA/SOUNDS/S0000009.A", &body, true),
        &lines(&body),
    );
    assert_eq!(said, "READY\nOK\n");
    clean(s.device());
}

/// The Sound kind's own check runs as the file comes: a project, or a
/// Sound it refuses, never shadows the pair.
#[test]
fn a_sound_put_takes_only_a_sound_the_check_passes() {
    let mut s = FatStore::new(fat16(16_384, 1));
    library(&mut s);
    let project = project_bytes("AVRIL 14", 1);
    let said = ask(
        &mut s,
        &sound_put("/CHIMERA/SOUNDS/S0000001.A", &project, true),
        &lines(&project),
    );
    assert_eq!(said, "READY\nERR put: WRONG FILE TYPE\n");
    // An empty Sound: no Engine record, which the check refuses.
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: None,
    };
    let mut empty = Bytes(Vec::new());
    write_file(&mut empty, &h, &mut |_| Ok(())).unwrap();
    let said = ask(
        &mut s,
        &sound_put("/CHIMERA/SOUNDS/S0000001.A", &empty.0, true),
        &lines(&empty.0),
    );
    assert!(said.starts_with("READY\nERR put: FILE "), "{said}");
    assert_eq!(on_card(&mut s, "S0000001.B"), None, "the torn side deleted");
    assert!(load_sound(&mut s, 1).unwrap().bits_eq(&sound_of(0)));
    clean(s.device());
}

#[test]
fn rm_deletes_pairs_and_torn_ones_and_refuses_factory_and_missing() {
    let mut s = FatStore::new(fat16(16_384, 1));
    library(&mut s);
    let mut unit = Fake::new(&mut s);
    let rm = |unit: &mut Fake<'_, _>, n: &str| {
        ask_unit(unit, &format!("rm /CHIMERA/SOUNDS/{n}\n"), b"", 0).0
    };
    assert_eq!(rm(&mut unit, "S0000001"), "OK\n");
    assert_eq!(unit.forgotten, 1);
    assert_eq!(rm(&mut unit, "s0000001"), "ERR rm: not found\n");
    assert_eq!(rm(&mut unit, "S0000077"), "ERR rm: not found\n");
    assert_eq!(rm(&mut unit, "S9000001"), "ERR rm: factory\n");
    overwrite(unit.files, "S0000002.A", b"");
    assert_eq!(rm(&mut unit, "S0000002"), "OK\n");
    for gone in ["S0000001.A", "S0000002.A", "S0000002.B"] {
        assert_eq!(on_card(&mut s, gone), None, "{gone}");
    }
    assert!(on_card(&mut s, "S9000001.A").is_some());
    clean(s.device());
}

/// `rm` on a card with no SOUNDS: not found, nothing made.
#[test]
fn rm_without_a_library_is_not_found() {
    let mut s = FatStore::new(fat16(16_384, 1));
    card(&mut s);
    assert_eq!(
        ask(&mut s, "rm /CHIMERA/SOUNDS/S0000001\n", b""),
        "ERR rm: not found\n"
    );
    assert_eq!(ask(&mut s, "ls sounds\n", b""), "ERR ls: FILE NOT FOUND\n");
}

/// `GET <len>`, its base64 lines, `CRC <hex>` and `OK`: the bytes and their CRC.
fn parse_get(said: &str) -> (Vec<u8>, u32) {
    let mut lines = said.lines();
    let len: usize = lines
        .next()
        .unwrap()
        .strip_prefix("GET ")
        .unwrap()
        .parse()
        .unwrap();
    let mut data = Vec::new();
    let mut crc_line = None;
    for l in lines.by_ref() {
        if let Some(c) = l.strip_prefix("CRC ") {
            crc_line = Some(u32::from_str_radix(c, 16).unwrap());
            break;
        }
        assert!(l.len() <= 64, "{l}");
        data.extend(unbase64(l));
    }
    assert_eq!(lines.next(), Some("OK"));
    assert_eq!(lines.next(), None);
    assert_eq!(data.len(), len);
    (data, crc_line.unwrap())
}

fn unbase64(l: &str) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let v = |c: u8| A.iter().position(|&a| a == c).map(|x| x as u32);
    let mut out = Vec::new();
    for q in l.as_bytes().chunks(4) {
        let n = q.iter().fold(0, |n, &c| n << 6 | v(c).unwrap_or(0));
        let k = q.iter().filter(|&&c| c != b'=').count() - 1;
        out.extend(&n.to_be_bytes()[1..1 + k]);
    }
    out
}

#[test]
fn get_streams_the_picked_side() {
    let mut s = FatStore::new(fat16(16_384, 1));
    library(&mut s);
    let (data, c) = parse_get(&ask(&mut s, "get /CHIMERA/SOUNDS/S0000001\n", b""));
    assert_eq!(data, on_card(&mut s, "S0000001.A").unwrap());
    assert_eq!(c, crc(&data));
    // The newer side of a pair; then, with it torn, the other.
    let (data, _) = parse_get(&ask(&mut s, "get /chimera/sounds/s0000002\n", b""));
    assert_eq!(data, on_card(&mut s, "S0000002.B").unwrap());
    let mut torn = data.clone();
    torn[40] ^= 1;
    overwrite(&mut s, "S0000002.B", &torn);
    let (data, _) = parse_get(&ask(&mut s, "get /CHIMERA/SOUNDS/S0000002\n", b""));
    assert_eq!(data, on_card(&mut s, "S0000002.A").unwrap());
}

#[test]
fn get_of_a_torn_pair_says_why() {
    let mut s = FatStore::new(fat16(16_384, 1));
    library(&mut s);
    overwrite(&mut s, "S0000002.A", b"");
    let mut b = on_card(&mut s, "S0000002.B").unwrap();
    b.truncate(b.len() - 10);
    overwrite(&mut s, "S0000002.B", &b);
    assert_eq!(
        ask(&mut s, "get /CHIMERA/SOUNDS/S0000002\n", b""),
        "ERR get: FILE IS CUT SHORT\n"
    );
    assert_eq!(
        ask(&mut s, "get /CHIMERA/SOUNDS/S0000003\n", b""),
        "ERR get: not found\n"
    );
}

const BANK2: &str = "/CHIMERA/SYSTEM/BANK2.BK2";

fn bank2_file() -> FileName {
    FileName::new(Dir::System, b"BANK2", b"BK2").unwrap()
}

fn system<S: Store>(s: &mut S, vol: VolumeId) -> Vec<(FileName, u32)> {
    let mut got = Vec::new();
    match s.list(vol, Dir::System, &mut |f, n| got.push((f, n))) {
        Ok(()) | Err(StoreError::NotFound) => got,
        Err(e) => panic!("{e:?}"),
    }
}

fn body(n: usize) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(13) ^ 0x5A).collect()
}

#[test]
fn a_bank2_image_is_staged_whole() {
    let mut s = FatStore::new(fat16(16_384, 1));
    let vol = card(&mut s);
    let img = bank2_image(&body(4352), console::SEAL);
    assert_eq!(img.len(), 4608);
    assert_eq!(
        ask(&mut s, &put_line(BANK2, &img), &lines(&img)),
        "READY\nOK\n"
    );
    assert_eq!(read(&mut s, vol, bank2_file()), Some(img.clone()));
    // Staged again, it replaces the old file.
    let new = bank2_image(&body(8192 - 256), console::SEAL);
    assert_eq!(
        ask(&mut s, &put_line(BANK2, &new), &lines(&new)),
        "READY\nOK\n"
    );
    assert_eq!(system(&mut s, vol), [(bank2_file(), 8192)]);
    assert_eq!(read(&mut s, vol, bank2_file()), Some(new));
    clean(s.device());
}

/// One for another build is kept: the boot after its firmware installs it.
#[test]
fn a_stale_bank2_image_is_kept_and_says_whose() {
    let mut s = FatStore::new(fat16(16_384, 1));
    let vol = card(&mut s);
    let img = bank2_image(&body(4352), *b"OTHERONE");
    assert_eq!(
        ask(&mut s, &put_line(BANK2, &img), &lines(&img)),
        "READY\nOK staged: needs firmware 0.2.0 02e440e2\n"
    );
    assert_eq!(read(&mut s, vol, bank2_file()), Some(img));
    clean(s.device());
}

#[test]
fn a_file_that_is_not_a_bank2_image_is_refused_and_gone() {
    let mut s = FatStore::new(fat16(16_384, 1));
    let vol = card(&mut s);
    let mut torn = bank2_image(&body(4352), console::SEAL);
    torn[300] ^= 1;
    let mut uncommitted = bank2_image(&body(4352), console::SEAL);
    uncommitted[0xE0] = 0xFF;
    for bad in [vec![0u8; 1024], torn, uncommitted, vec![0xFF; 512]] {
        assert_eq!(
            ask(&mut s, &put_line(BANK2, &bad), &lines(&bad)),
            "READY\nERR put: not a bank-2 image\n"
        );
        assert_eq!(system(&mut s, vol), []);
    }
    clean(s.device());
}

#[test]
fn an_unsealed_unit_stages_nothing() {
    let mut s = FatStore::new(fat16(16_384, 1));
    let vol = card(&mut s);
    let img = bank2_image(&body(4352), console::SEAL);
    let mut unit = Fake::new(&mut s);
    unit.seal = None;
    let (said, _) = ask_unit(&mut unit, &put_line(BANK2, &img), &lines(&img), 0);
    assert_eq!(said, "ERR put: firmware unsealed\n");
    assert_eq!(system(&mut s, vol), []);
}

/// A bad transfer leaves no staged file, whatever was there.
#[test]
fn a_failed_bank2_transfer_leaves_nothing() {
    let mut s = FatStore::new(fat16(16_384, 1));
    let vol = card(&mut s);
    let img = bank2_image(&body(4352), console::SEAL);
    let line = format!("put {BANK2} {} {:08x}\n", img.len(), crc(&img) ^ 1);
    assert_eq!(
        ask(&mut s, &line, &lines(&img)),
        "READY\nERR put: CRC mismatch\n"
    );
    assert_eq!(system(&mut s, vol), []);
    clean(s.device());
}

/// `img` over a planted file's chain.
fn fill_chain(t: &tree::Tree, chain: &[u32], img: &[u8]) {
    let per = t.l.blocks_per_cluster();
    let blocks = chain
        .iter()
        .flat_map(|&c| (0..per).map(move |i| t.l.cluster_block(c).unwrap() + i));
    let mut d = t.disk.0.borrow_mut();
    for (b, part) in blocks.zip(img.chunks(512)) {
        d[(image::PART_LBA + b) as usize][..part.len()].copy_from_slice(part);
    }
}

/// `chimera-0.2.0.bk2` dropped in the root by a host, as its short name
/// `CHIMER~2.BK2` after a long-name run: the boot finds it, past one for
/// another build and the `.bin` beside it.
#[test]
fn the_boot_finds_a_bk2_dropped_in_the_root() {
    let disk = fat16(16_384, 1);
    let good = bank2_image(&body(4352), console::SEAL);
    let stale = bank2_image(&body(4352), *b"SEAL0002");
    {
        let mut t = tree::Tree::new(&disk);
        let mut root = t.root();
        for (short, long, img) in [
            (b"CHIMER~1BK2", "chimera-0.1.0.bk2", &stale),
            (b"CHIMER~2BK2", "chimera-0.2.0.bk2", &good),
        ] {
            let f = t.file(
                &mut root,
                *short,
                Some(long),
                (img.len() as u32, 0),
                1,
                None,
            );
            fill_chain(&t, &f.chain, img);
        }
        t.file(
            &mut root,
            *b"CHIMER~3BIN",
            Some("chimera-0.2.0.bin"),
            (900, 0),
            2,
            None,
        );
    }
    let mut s = FatStore::new(disk);
    let seal = chimera_image::Seal::new(console::SEAL).unwrap();
    let (img, h) = find_staged(&mut s, &chimera_image::Verdict::Empty, &seal).expect("found");
    assert_eq!(
        img.file(),
        FileName::new(Dir::Root, b"CHIMER~2", b"BK2").unwrap()
    );
    assert_eq!(h.version.as_str(), "0.2.0 02e440e2");
    let vol = s.mount().unwrap();
    let mut root = Vec::new();
    s.list(vol, Dir::Root, &mut |f, n| root.push((f, n)))
        .unwrap();
    assert_eq!(root.len(), 3, "{root:?}");
}
