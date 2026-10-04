//! The console's `put` and `ls` on a FAT card in RAM, through `answer` as
//! the shells drive them: a project file saved the unit's way, every
//! failure leaving the pair loading as before, with no cluster lost.

#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;
#[path = "common/tools.rs"]
mod tools;

use chimera_core::console::{
    CardAnswer, Console, Frame, Out, PUT_DEADLINE_MS, PUT_MAX_BYTES, PutArg, Stalled, Stats, Unit,
    answer, put,
};
use chimera_core::name::Name;
use chimera_core::project::test_support::full_dense;
use chimera_core::project::{
    Project, ProjectDecoder, encode_new_project, encode_project, list_projects, project_file,
};
use chimera_core::storage::{
    Card, Crc32, FileKind, Generation, Header, ProjectId, RecordWriter, Side, load_ab_in_place,
    peek_header, write_file,
};
use chimera_core::ui::UiState;
use chimera_core::ui::theme_settings::ThemeSettings;
use chimera_fat::FatStore;
use chimera_hal::store::{ByteSink, Dir, FileName, Files, Store, StoreError, VolumeId};
use chimera_hal::{FB_SIZE, Ms};
use image::{Cut, CutDisk, RamDisk, fat_check, fat16, with_clusters};
use probe::{Probed, Sink, log, probed};
use std::collections::VecDeque;
use std::rc::Rc;

struct Fake<'a, S> {
    ui: Box<UiState>,
    fb: Box<[u16; FB_SIZE]>,
    files: &'a mut S,
}

impl<S: Files> Unit for Fake<'_, S> {
    fn ui(&self) -> &UiState {
        &self.ui
    }
    fn stats(&mut self) -> Option<Stats> {
        None
    }
    fn bench(&self) -> Option<&str> {
        None
    }
    fn card(&mut self) -> Option<CardAnswer> {
        None
    }
    fn frame(&self) -> Frame<'_> {
        Frame {
            fb: &self.fb,
            palette: ThemeSettings::DEFAULT.palette(),
        }
    }
    fn dfu(&mut self) -> Option<()> {
        None
    }
    fn boot(&self) -> Option<chimera_core::boot::BootSeen> {
        None
    }
    fn usb_regs(&self) -> Option<chimera_core::boot::UsbRegs> {
        None
    }
    type Files = S;
    fn files(&mut self) -> Option<&mut S> {
        Some(&mut *self.files)
    }
}

/// The host: what it sends after the request line, and a clock in µs that
/// moves `per_byte_us` a byte and the whole wait when nothing comes.
struct Host {
    input: VecDeque<u8>,
    out: Vec<u8>,
    us: u64,
    per_byte_us: u64,
}

impl Host {
    fn new(data: &[u8], per_byte_us: u64) -> Host {
        Host {
            input: data.iter().copied().collect(),
            out: Vec::new(),
            us: 0,
            per_byte_us,
        }
    }
}

impl Out for Host {
    fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
        self.out.extend_from_slice(b);
        Ok(())
    }
    fn recv(&mut self, wait_ms: u32) -> Option<u8> {
        let b = self.input.pop_front();
        self.us += match b {
            Some(_) => self.per_byte_us,
            None => u64::from(wait_ms) * 1_000,
        };
        b
    }
    fn now(&self) -> Ms {
        Ms((self.us / 1_000) as u32)
    }
}

/// `line`, then `data`, through `answer`: what the unit said, and what of
/// the host's data it left unread.
fn ask_on<S: Files>(s: &mut S, line: &str, data: &[u8], per_byte_us: u64) -> (String, usize) {
    let mut unit = Fake {
        ui: Box::new(UiState::new()),
        fb: Box::new([0; FB_SIZE]),
        files: s,
    };
    let mut host = Host::new(data, per_byte_us);
    let mut c = Console::new();
    for &b in line.as_bytes() {
        if let Some(r) = c.push(b) {
            answer(r, &mut unit, &mut host).unwrap();
        }
    }
    (String::from_utf8(host.out).unwrap(), host.input.len())
}

fn ask<S: Files>(s: &mut S, line: &str, data: &[u8]) -> String {
    ask_on(s, line, data, 0).0
}

fn base64(b: &[u8]) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for c in b.chunks(3) {
        let n = c
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, &x)| n | u32::from(x) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= c.len() {
                A[(n >> (18 - 6 * i) & 63) as usize]
            } else {
                b'='
            });
        }
    }
    out
}

/// The data as the host tool sends it: 48 bytes a line.
fn lines(b: &[u8]) -> Vec<u8> {
    b.chunks(48)
        .flat_map(|c| [base64(c), b"\n".to_vec()].concat())
        .collect()
}

fn crc(b: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(b);
    c.finish()
}

const ID: u32 = 14;
const PATH: &str = "/CHIMERA/PROJECTS/P0000014.A";

fn put_line(path: &str, body: &[u8]) -> String {
    format!("put {path} {} {:08x}\n", body.len(), crc(body))
}

struct Bytes(Vec<u8>);

impl ByteSink for Bytes {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        self.0.extend_from_slice(b);
        Ok(())
    }
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
    let body = project_bytes("AVRIL 14", 1);
    assert!(
        body.len() % 48 > 1,
        "the last line carries more than one byte"
    );
    // Declared a byte short of what the last line carries.
    let line = format!("put {PATH} {} 0\n", body.len() - 1);
    fails_after_ready(line, &lines(&body), 0, "more data than its size");
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
    let usage = "ERR put takes /CHIMERA/PROJECTS/P#######.A, its size and CRC-32 in hex\n";
    for path in [
        "/CHIMERA/PROJECTS/AVRIL14.A",
        "/CHIMERA/PROJECTS/../SYSTEM.A",
        "/CHIMERA/PROJECTS//P0000014.A",
        "/CHIMERA/PROJECTS/",
        "/CHIMERA/SOUNDS/S0000001.A",
        "/CHIMERA/SYSTEM.A",
        "/CHIMERA/BENCH/CARD0.BIN",
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
        to: ProjectId::new(ID).unwrap(),
        size: body.len() as u32,
        crc: crc(&body),
    };
    let mut host = Host::new(&lines(&body), 0);
    put(arg, ProjectId::new(ID), &mut s, &mut host).unwrap();
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
