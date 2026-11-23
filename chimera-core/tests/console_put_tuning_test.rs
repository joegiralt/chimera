// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `put` takes a tuning (#217, ADR 0240, 0259): `/CHIMERA/TUNINGS/<stem>.SCL`
//! or `.KBM`, any 8.3 stem, a whole file of at most 4 KiB, replacing an old
//! one; the card is inside the case, so this is how a Scala file gets there.
#![cfg(all(feature = "card-put", feature = "tuning"))]

use chimera_core::console::{Console, Out, Request, Stalled, put, rm};
use chimera_core::storage::Crc32;
use chimera_hal::Ms;
use chimera_hal::store::{ByteSink, Dir, FileName, Files, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use core::ops::ControlFlow;
use std::collections::VecDeque;

struct Mem(MemStore);

impl Store for Mem {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.0.mount()
    }
    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.0.list(vol, dir, f)
    }
    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        s: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.0.read(vol, file, s)
    }
    fn read_at(
        &mut self,
        vol: VolumeId,
        file: FileName,
        offset: u32,
        s: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.0.read_at(vol, file, offset, s)
    }
    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.0.write(vol, file, body)
    }
    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.0.delete(vol, file)
    }
    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.0.make_dir(vol, dir)
    }
    fn seal(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.0.seal(vol, file)
    }
}

impl Files for Mem {
    fn cap(&mut self, _ms: Option<u32>) {}
}

struct Host {
    input: VecDeque<u8>,
    out: Vec<u8>,
    us: u64,
}

impl Out for Host {
    fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
        self.out.extend_from_slice(b);
        Ok(())
    }
    fn recv(&mut self, wait_ms: u32) -> Option<u8> {
        let b = self.input.pop_front();
        self.us += if b.is_some() {
            50
        } else {
            u64::from(wait_ms) * 1_000
        };
        b
    }
    fn now(&self) -> Ms {
        Ms((self.us / 1_000) as u32)
    }
}

fn crc(b: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(b);
    c.finish()
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

/// A `put` of `data` to `path`: the unit's answer, or the parser's refusal.
fn ask(s: &mut Mem, path: &str, data: &[u8]) -> String {
    let input = data
        .chunks(48)
        .flat_map(|c| [base64(c), b"\n".to_vec()].concat());
    let mut host = Host {
        input: input.collect(),
        out: Vec::new(),
        us: 0,
    };
    let line = format!("put {path} {} {:08x}\n", data.len(), crc(data));
    let mut c = Console::new();
    for &b in line.as_bytes() {
        match c.push(b) {
            Some(Ok(Request::Put(arg))) => put(arg, None, None, s, &mut host).unwrap(),
            Some(other) => return format!("{other:?}"),
            None => {}
        }
    }
    String::from_utf8(host.out).unwrap()
}

fn on_card(s: &mut Mem, stem: &[u8], ext: &[u8]) -> Option<Vec<u8>> {
    struct Sink(Vec<u8>);
    impl ReadSink for Sink {
        fn begin(&mut self, _: u32) -> ControlFlow<()> {
            ControlFlow::Continue(())
        }
        fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
            self.0.extend_from_slice(b);
            ControlFlow::Continue(())
        }
    }
    let vol = s.mount().unwrap();
    let mut k = Sink(Vec::new());
    let f = FileName::new(Dir::Tunings, stem, ext).unwrap();
    s.read(vol, f, &mut k).ok().map(|()| k.0)
}

const SCL: &[u8] = b"! pelog\nPelog\n7\n120.\n270.\n540.\n670.\n785.\n950.\n2/1\n";

#[test]
fn put_takes_a_scala_file_and_a_map() {
    let mut s = Mem(MemStore::new(7));
    let was = chimera_core::tuning::files_put();
    assert_eq!(
        ask(&mut s, "/CHIMERA/TUNINGS/TUNE3.SCL", SCL),
        "READY\nOK\n"
    );
    assert_eq!(on_card(&mut s, b"TUNE3", b"SCL").as_deref(), Some(SCL));
    assert_ne!(
        chimera_core::tuning::files_put(),
        was,
        "the UI reads it again"
    );
    let kbm = b"0\n0\n127\n60\n69\n432.0\n7\n";
    assert_eq!(
        ask(&mut s, "/chimera/tunings/tune3.kbm", kbm),
        "READY\nOK\n"
    );
    assert_eq!(on_card(&mut s, b"TUNE3", b"KBM").as_deref(), Some(&kbm[..]));
    // A second put replaces it.
    assert_eq!(
        ask(&mut s, "/CHIMERA/TUNINGS/TUNE3.SCL", b"x\n1\n2/1\n"),
        "READY\nOK\n"
    );
    assert_eq!(
        on_card(&mut s, b"TUNE3", b"SCL").as_deref(),
        Some(&b"x\n1\n2/1\n"[..])
    );
}

#[test]
fn put_takes_any_stem_and_refuses_other_names_and_a_file_past_4_kib() {
    let mut s = Mem(MemStore::new(7));
    assert_eq!(
        ask(&mut s, "/chimera/tunings/bohlen13.scl", SCL),
        "READY\nOK\n"
    );
    assert_eq!(on_card(&mut s, b"BOHLEN13", b"SCL").as_deref(), Some(SCL));
    for path in [
        "/CHIMERA/TUNINGS/BOHLENPIERCE.SCL",
        "/CHIMERA/TUNINGS/BO-HL.SCL",
        "/CHIMERA/TUNINGS/BOHLEN~1.SCL",
        "/CHIMERA/MAPS/FADER~1.A",
        "/CHIMERA/TUNINGS/.SCL",
        "/CHIMERA/TUNINGS/TUNE1.TXT",
        "/CHIMERA/MAPS/TUNE1.SCL",
    ] {
        assert_eq!(ask(&mut s, path, SCL), "Err(Arguments(Put))", "{path}");
    }
    let big = vec![b'!'; 4 * 1024 + 1];
    assert_eq!(
        ask(&mut s, "/CHIMERA/TUNINGS/TUNE1.SCL", &big),
        "Err(Arguments(Put))"
    );
    assert_eq!(on_card(&mut s, b"TUNE1", b"SCL"), None);
}

#[test]
fn a_scale_put_drops_its_slots_old_map() {
    let mut s = Mem(MemStore::new(7));
    let kbm = b"0\n0\n127\n60\n69\n432.0\n7\n";
    assert_eq!(
        ask(&mut s, "/CHIMERA/TUNINGS/TUNE4.SCL", SCL),
        "READY\nOK\n"
    );
    assert_eq!(
        ask(&mut s, "/CHIMERA/TUNINGS/TUNE4.KBM", kbm),
        "READY\nOK\n"
    );
    // A new scale in the slot: the old map would stay applied unseen.
    assert_eq!(
        ask(&mut s, "/CHIMERA/TUNINGS/TUNE4.SCL", SCL),
        "READY\nOK\n"
    );
    assert_eq!(on_card(&mut s, b"TUNE4", b"KBM"), None);
    assert_eq!(on_card(&mut s, b"TUNE4", b"SCL").as_deref(), Some(SCL));
    // A map put keeps its scale; another slot's map is untouched.
    assert_eq!(
        ask(&mut s, "/CHIMERA/TUNINGS/TUNE4.KBM", kbm),
        "READY\nOK\n"
    );
    assert_eq!(
        ask(&mut s, "/CHIMERA/TUNINGS/TUNE5.SCL", SCL),
        "READY\nOK\n"
    );
    assert_eq!(on_card(&mut s, b"TUNE4", b"KBM").as_deref(), Some(&kbm[..]));
    assert_eq!(on_card(&mut s, b"TUNE4", b"SCL").as_deref(), Some(SCL));
}

/// An `rm` line: the unit's answer, or the parser's refusal.
fn ask_rm(s: &mut Mem, path: &str) -> String {
    let mut host = Host {
        input: VecDeque::new(),
        out: Vec::new(),
        us: 0,
    };
    let mut c = Console::new();
    for &b in format!("rm {path}\n").as_bytes() {
        match c.push(b) {
            Some(Ok(Request::Rm(arg))) => rm(arg, s, &mut host).unwrap(),
            Some(other) => return format!("{other:?}"),
            None => {}
        }
    }
    String::from_utf8(host.out).unwrap()
}

/// `rm` takes a tuning file put there (#906): the card is inside the case,
/// so a file `put` can always be taken off, under any 8.3 stem (ADR 0259).
/// The Parts playing it read again (and play 12-TET with the file gone).
#[test]
fn rm_takes_a_tuning_file() {
    let mut s = Mem(MemStore::new(7));
    let kbm = b"0\n0\n127\n60\n69\n432.0\n7\n";
    assert_eq!(
        ask(&mut s, "/CHIMERA/TUNINGS/PELOG.SCL", SCL),
        "READY\nOK\n"
    );
    assert_eq!(
        ask(&mut s, "/CHIMERA/TUNINGS/PELOG.KBM", kbm),
        "READY\nOK\n"
    );
    let was = chimera_core::tuning::files_put();
    assert_eq!(ask_rm(&mut s, "/chimera/tunings/pelog.kbm"), "OK\n");
    assert_eq!(on_card(&mut s, b"PELOG", b"KBM"), None);
    assert_eq!(on_card(&mut s, b"PELOG", b"SCL").as_deref(), Some(SCL));
    assert_ne!(
        chimera_core::tuning::files_put(),
        was,
        "the UI reads it again"
    );
    assert_eq!(ask_rm(&mut s, "/CHIMERA/TUNINGS/PELOG.SCL"), "OK\n");
    assert_eq!(on_card(&mut s, b"PELOG", b"SCL"), None);
    assert_eq!(
        ask_rm(&mut s, "/CHIMERA/TUNINGS/PELOG.SCL"),
        "ERR rm: not found\n"
    );
    for path in [
        "/CHIMERA/TUNINGS/BOHLEN-P.SCL",
        "/CHIMERA/TUNINGS/PELOGSLEN.KBM",
        "/CHIMERA/TUNINGS/._PELOG.SCL",
        "/CHIMERA/TUNINGS/.SCL",
        "/CHIMERA/TUNINGS/TUNE1.TXT",
        "/CHIMERA/TUNINGS/TUNE1",
        "/CHIMERA/TUNINGS/../SOUNDS/TUNE1.SCL",
        "/CHIMERA/TUNINGS/TUNE1.SCL/..",
    ] {
        assert_eq!(ask_rm(&mut s, path), "Err(Arguments(Rm))", "{path}");
    }
}

/// `rm` also takes a computer's short name for a long one, as the card
/// keeps a file copied in USB disk mode (ADR 0259).
#[test]
fn rm_takes_a_long_names_short_name() {
    let mut s = Mem(MemStore::new(7));
    let f = FileName::new(Dir::Tunings, b"BOHLEN~1", b"SCL").unwrap();
    s.0.plant(f, SCL);
    assert_eq!(ask_rm(&mut s, "/CHIMERA/TUNINGS/bohlen~1.scl"), "OK\n");
    assert_eq!(on_card(&mut s, b"BOHLEN~1", b"SCL"), None);
}
