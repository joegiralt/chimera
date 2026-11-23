// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! `put` takes the bench's check table: one whole file of at most 9 KiB at
//! `/CHIMERA/BENCH/BEDS.CHK`, replacing an old one.
#![cfg(feature = "card-put")]

use chimera_core::console::{Console, Out, Request, Stalled, put};
use chimera_core::storage::Crc32;
use chimera_core::stream::bench::{BedCheck, BedChecks};
use chimera_hal::Ms;
use chimera_hal::store::{ByteSink, Dir, FileName, Files, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use core::ops::ControlFlow;
use std::collections::VecDeque;

struct Mem(MemStore);

impl Store for Mem {
    fn clear_read_fault(&mut self) {}
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

fn beds() -> FileName {
    FileName::new(Dir::Bench, b"BEDS", b"CHK").unwrap()
}

fn on_card(s: &mut Mem) -> Option<Vec<u8>> {
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
    s.read(vol, beds(), &mut k).ok().map(|()| k.0)
}

/// A `BEDS.CHK` of one bed of `chunks` chunks.
fn table(stem: u32, chunks: u32) -> Vec<u8> {
    let crcs: Vec<u32> = (0..chunks).map(|c| c * 31 + stem).collect();
    let all = BedChecks::new(&[BedCheck::new(stem, &crcs).unwrap()]).unwrap();
    let mut out = vec![0; 9 * 1024];
    let n = all.encode(&mut out);
    out.truncate(n);
    out
}

#[test]
fn put_takes_the_bed_checks() {
    let mut s = Mem(MemStore::new(7));
    let first = table(1, 9);
    let said = ask(&mut s, "/CHIMERA/BENCH/BEDS.CHK", &first);
    assert_eq!(said, "READY\nOK\n");
    assert_eq!(on_card(&mut s).as_deref(), Some(&first[..]));
    // A second put replaces it.
    let second = table(2, 200);
    let said = ask(&mut s, "/chimera/bench/beds.chk", &second);
    assert_eq!(said, "READY\nOK\n");
    assert_eq!(on_card(&mut s).as_deref(), Some(&second[..]));
}

#[test]
fn put_refuses_other_bench_files_and_an_oversized_table() {
    let mut s = Mem(MemStore::new(7));
    let said = ask(&mut s, "/CHIMERA/BENCH/OTHER.CHK", &[1; 64]);
    assert_eq!(said, "Err(Arguments(Put))");
    let said = ask(&mut s, "/CHIMERA/BENCH/BEDS.CHK", &vec![1; 9 * 1024 + 1]);
    assert_eq!(said, "Err(Arguments(Put))");
    assert_eq!(on_card(&mut s), None);
}
