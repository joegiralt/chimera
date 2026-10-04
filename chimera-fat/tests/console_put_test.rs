//! The console's `put` and `ls` on a FAT card in RAM: through `answer`, as
//! the shells drive them, so the parse, the transfer and the card meet.

#[path = "common/image.rs"]
mod image;
#[path = "common/probe.rs"]
mod probe;

use chimera_core::console::{
    CardAnswer, Console, Frame, Out, PUT_DEADLINE_MS, PUT_MAX_BYTES, Stalled, Stats, Unit, answer,
};
use chimera_core::storage::Crc32;
use chimera_core::ui::UiState;
use chimera_core::ui::theme_settings::ThemeSettings;
use chimera_fat::FatStore;
use chimera_hal::store::{Dir, FileName, Files, Store, VolumeId};
use chimera_hal::{FB_SIZE, Ms};
use image::{RamDisk, fat16, pattern, with_clusters};
use probe::Sink;
use std::collections::VecDeque;

type Card = FatStore<RamDisk>;

struct Fake<'a> {
    ui: Box<UiState>,
    fb: Box<[u16; FB_SIZE]>,
    files: &'a mut Card,
}

impl Unit for Fake<'_> {
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
    fn files(&mut self) -> Option<&mut dyn Files> {
        Some(&mut *self.files)
    }
}

/// The host: what it will send after the request line, and a clock that
/// moves `per_byte_ms` a byte and the whole wait when nothing comes.
struct Host {
    input: VecDeque<u8>,
    out: Vec<u8>,
    now: u32,
    per_byte_ms: u32,
}

impl Out for Host {
    fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
        self.out.extend_from_slice(b);
        Ok(())
    }
    fn recv(&mut self, wait_ms: u32) -> Option<u8> {
        let b = self.input.pop_front();
        self.now += if b.is_some() {
            self.per_byte_ms
        } else {
            wait_ms
        };
        b
    }
    fn now(&self) -> Ms {
        Ms(self.now)
    }
}

/// `line`, then `data`, through `answer`: what the unit said.
fn ask_slow(card: &mut Card, line: &str, data: &[u8], per_byte_ms: u32) -> String {
    let mut unit = Fake {
        ui: Box::new(UiState::new()),
        fb: Box::new([0; FB_SIZE]),
        files: card,
    };
    let mut host = Host {
        input: data.iter().copied().collect(),
        out: Vec::new(),
        now: 0,
        per_byte_ms,
    };
    let mut c = Console::new();
    for &b in line.as_bytes() {
        if let Some(r) = c.push(b) {
            answer(r, &mut unit, &mut host).unwrap();
        }
    }
    String::from_utf8(host.out).unwrap()
}

fn ask(card: &mut Card, line: &str, data: &[u8]) -> String {
    ask_slow(card, line, data, 0)
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

fn put_line(path: &str, body: &[u8]) -> String {
    format!("put {path} {} {:08x}\n", body.len(), crc(body))
}

fn card(blocks: u32) -> (Card, VolumeId) {
    let mut s = FatStore::new(fat16(blocks, 1));
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    (s, vol)
}

fn project(stem: &[u8]) -> FileName {
    FileName::new(Dir::Projects, stem, b"A").unwrap()
}

fn read(s: &mut Card, vol: VolumeId, f: FileName) -> Vec<u8> {
    let mut sink = Sink::default();
    s.read(vol, f, &mut sink).unwrap();
    sink.0
}

fn names(s: &mut Card, vol: VolumeId, dir: Dir) -> Vec<(FileName, u32)> {
    let mut v = Vec::new();
    let _ = s.list(vol, dir, &mut |f, n| v.push((f, n)));
    v
}

/// No temporary file in either directory, and every cluster back.
fn clean(s: &mut Card, vol: VolumeId, free: u64) {
    for d in [Dir::Projects, Dir::Sounds] {
        let tmp = names(s, vol, d)
            .into_iter()
            .find(|(f, _)| f.stem() == b"PUT");
        assert_eq!(tmp, None, "no temporary file in {d:?}");
    }
    assert_eq!(Files::free_bytes(s, vol).unwrap(), free, "no cluster lost");
}

const PATH: &str = "/CHIMERA/PROJECTS/P0000014.A";

#[test]
fn a_good_put_lands_and_ls_shows_it() {
    let (mut s, vol) = card(16_384);
    let body = pattern(3, 130_000);
    let said = ask(&mut s, &put_line(PATH, &body), &lines(&body));
    assert_eq!(said, "READY\nOK\n");
    assert_eq!(read(&mut s, vol, project(b"P0000014")), body);
    let free = Files::free_bytes(&mut s, vol).unwrap();
    clean(&mut s, vol, free);
    assert_eq!(
        ask(&mut s, "ls /chimera/projects\n", b""),
        "P0000014.A 130000\nOK\n"
    );
}

#[test]
fn a_put_replaces_the_target_and_frees_its_old_clusters() {
    let (mut s, vol) = card(16_384);
    let empty = Files::free_bytes(&mut s, vol).unwrap();
    let old = pattern(1, 200_000);
    assert_eq!(
        ask(&mut s, &put_line(PATH, &old), &lines(&old)),
        "READY\nOK\n"
    );
    let new = pattern(2, 5_000);
    // Lower case, and a CRC without leading zeros, as a person types them.
    let line = format!(
        "put /chimera/projects/p0000014.a {} {:x}\n",
        new.len(),
        crc(&new)
    );
    assert_eq!(ask(&mut s, &line, &lines(&new)), "READY\nOK\n");
    assert_eq!(read(&mut s, vol, project(b"P0000014")), new);
    // The PROJECTS directory's cluster and the new file's 10 (FAT16, 512 B).
    clean(&mut s, vol, empty - 512 - 10 * 512);
}

#[test]
fn a_leftover_temporary_file_is_written_over() {
    let (mut s, vol) = card(16_384);
    s.make_dir(vol, Dir::Projects).unwrap();
    let free = Files::free_bytes(&mut s, vol).unwrap();
    let tmp = FileName::new(Dir::Projects, b"PUT", b"TMP").unwrap();
    s.write(vol, tmp, &mut |w| w.put(&[7; 3_000])).unwrap();
    let body = pattern(4, 1_000);
    assert_eq!(
        ask(&mut s, &put_line(PATH, &body), &lines(&body)),
        "READY\nOK\n"
    );
    assert_eq!(read(&mut s, vol, project(b"P0000014")), body);
    clean(&mut s, vol, free - 2 * 512);
}

/// Each failure: its `ERR`, the target as it was, no temporary file and
/// no cluster lost.
fn fails(line: String, data: &[u8], per_byte_ms: u32, err: &str, ready: bool) {
    let (mut s, vol) = card(16_384);
    let old = pattern(9, 2_000);
    assert_eq!(
        ask(&mut s, &put_line(PATH, &old), &lines(&old)),
        "READY\nOK\n"
    );
    let free = Files::free_bytes(&mut s, vol).unwrap();
    let said = ask_slow(&mut s, &line, data, per_byte_ms);
    let want = format!("{}ERR put: {err}\n", if ready { "READY\n" } else { "" });
    assert_eq!(said, want);
    assert_eq!(
        read(&mut s, vol, project(b"P0000014")),
        old,
        "the target untouched"
    );
    clean(&mut s, vol, free);
}

#[test]
fn a_bad_crc_is_refused() {
    let body = pattern(5, 70_000);
    let line = format!("put {PATH} {} {:08x}\n", body.len(), crc(&body) ^ 1);
    fails(line, &lines(&body), 0, "CRC mismatch", true);
}

#[test]
fn a_short_transfer_times_out() {
    let body = pattern(5, 70_000);
    let half = lines(&body[..35_040]);
    fails(put_line(PATH, &body), &half, 0, "no data for 500 ms", true);
}

#[test]
fn an_abort_mid_way_stops_it() {
    let body = pattern(5, 70_000);
    let mut data = lines(&body[..20_016]);
    data.extend_from_slice(b"abort\n");
    data.extend_from_slice(&lines(&body[20_016..]));
    fails(put_line(PATH, &body), &data, 0, "aborted", true);
}

#[test]
fn more_data_than_the_size_is_refused() {
    let body = pattern(5, 1_000);
    let line = format!("put {PATH} 999 {:08x}\n", crc(&body[..999]));
    fails(line, &lines(&body), 0, "more data than its size", true);
}

#[test]
fn a_bad_line_is_refused() {
    let body = pattern(5, 1_000);
    for bad in [
        &b"not base64!\n"[..],
        b"QUJD=QUJD\n",
        b"QUJ\n",
        &[b'A'; 68],
        b"QQ==QUJD\n",
    ] {
        fails(put_line(PATH, &body), bad, 0, "bad data line", true);
    }
}

#[test]
fn a_put_past_the_deadline_times_out() {
    let body = pattern(5, 60_000);
    // 1 ms a byte: 80 000 base64 bytes don't fit in the deadline.
    assert!(lines(&body).len() as u32 > PUT_DEADLINE_MS);
    fails(put_line(PATH, &body), &lines(&body), 1, "timeout", true);
}

#[test]
fn a_file_over_the_limit_is_refused_before_ready() {
    let line = format!("put {PATH} {} 0\n", PUT_MAX_BYTES + 1);
    fails(line, b"", 0, "over 1 MiB", false);
}

#[test]
fn paths_outside_projects_and_sounds_are_refused() {
    let (mut s, vol) = card(16_384);
    let free = Files::free_bytes(&mut s, vol).unwrap();
    for path in [
        "/CHIMERA/PROJECTS/../SYSTEM.A",
        "/CHIMERA/SYSTEM.A",
        "/CHIMERA/PROJECTS/..",
        "/CHIMERA/PROJECTS/.A",
        "/CHIMERA/BENCH/CARD0.BIN",
        "/CHIMERA/PROJECTS/SUB/P1.A",
        "/OTHER/P1.A",
        "PROJECTS/P1.A",
        "/CHIMERA/PROJECTS/PUT.TMP",
        "/CHIMERA/PROJECTS/TOOLONGNAME.A",
        "/CHIMERA/PROJECTS/P1.ABCD",
        "/CHIMERA//PROJECTS/P1.A",
    ] {
        assert_eq!(
            ask(&mut s, &format!("put {path} 1 0\n"), b"QQ==\n"),
            "ERR put takes /CHIMERA/PROJECTS/<8.3> or /CHIMERA/SOUNDS/<8.3>, its size and CRC-32 in hex\n",
            "{path}"
        );
    }
    for line in [
        "put",
        "put /CHIMERA/SOUNDS/S1.A",
        "put /CHIMERA/SOUNDS/S1.A 1",
        "put /CHIMERA/SOUNDS/S1.A x 0",
        "put /CHIMERA/SOUNDS/S1.A 1 123456789",
        "put /CHIMERA/SOUNDS/S1.A 1 0 more",
    ] {
        assert!(
            ask(&mut s, &format!("{line}\n"), b"").starts_with("ERR put takes"),
            "{line}"
        );
    }
    assert_eq!(names(&mut s, vol, Dir::Projects), []);
    assert_eq!(
        Files::free_bytes(&mut s, vol).unwrap(),
        free,
        "nothing written"
    );
    // A Sound goes in SOUNDS.
    let body = pattern(6, 300);
    assert_eq!(
        ask(
            &mut s,
            &put_line("/CHIMERA/SOUNDS/S0000002.B", &body),
            &lines(&body)
        ),
        "READY\nOK\n"
    );
}

#[test]
fn a_full_card_is_refused_before_ready() {
    // FAT16's fewest clusters, about 2 MB free: 1 MiB would leave under
    // the 1 MiB margin.
    let mut s = FatStore::new(with_clusters(4_085, 1));
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    let free = Files::free_bytes(&mut s, vol).unwrap();
    let body = pattern(7, PUT_MAX_BYTES as usize);
    assert!(free > 2 * 1_000_000 && free < 2 << 20);
    assert_eq!(
        ask(&mut s, &put_line(PATH, &body), &lines(&body)),
        "ERR put: would leave under 1 MiB free\n"
    );
    // The PROJECTS directory is made before the check: its one cluster.
    clean(&mut s, vol, free - 512);
}

#[test]
fn an_empty_file_puts() {
    let (mut s, vol) = card(16_384);
    assert_eq!(
        ask(&mut s, &format!("put {PATH} 0 0\n"), b""),
        "READY\nOK\n"
    );
    assert_eq!(read(&mut s, vol, project(b"P0000014")), b"");
}

#[test]
fn ls_lists_names_and_sizes_and_refuses_what_is_not_there() {
    let (mut s, vol) = card(16_384);
    assert_eq!(
        ask(&mut s, "ls /CHIMERA/SOUNDS\n", b""),
        "ERR ls: FILE NOT FOUND\n"
    );
    s.make_dir(vol, Dir::Sounds).unwrap();
    for (stem, n) in [(&b"S0000001"[..], 10), (b"S0000002", 2_000)] {
        let f = FileName::new(Dir::Sounds, stem, b"A").unwrap();
        s.write(vol, f, &mut |w| w.put(&vec![0; n])).unwrap();
    }
    let f = FileName::new(Dir::Sounds, b"README", b"").unwrap();
    s.write(vol, f, &mut |w| w.put(b"hi")).unwrap();
    assert_eq!(
        ask(&mut s, "ls /CHIMERA/SOUNDS/\n", b""),
        "S0000001.A 10\nS0000002.A 2000\nREADME 2\nOK\n"
    );
    assert_eq!(ask(&mut s, "ls /CHIMERA\n", b""), "OK\n");
    assert!(ask(&mut s, "ls /CHIMERA/..\n", b"").starts_with("ERR ls takes"));
    assert!(ask(&mut s, "ls\n", b"").starts_with("ERR ls takes"));
}
