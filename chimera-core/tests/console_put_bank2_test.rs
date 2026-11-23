// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! N5: a whole 1 MiB bank-2 image `put` at 16 KiB/s lands inside
//! `PUT_DEADLINE_MS`, on a host clock that runs as the bytes arrive.
#![cfg(feature = "card-put")]

use chimera_core::console::{Console, Out, PUT_DEADLINE_MS, Request, Stalled, put};
use chimera_hal::Ms;
use chimera_hal::store::{ByteSink, Dir, FileName, Files, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use chimera_image::crc::Crc32;
use chimera_image::{BANK2_LEN, Header, IMAGE_BODY, Seal, Version};
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

/// A host sending base64 at `us_per_char`; a wait for nothing passes in full.
struct Host {
    input: VecDeque<u8>,
    out: Vec<u8>,
    us: u64,
    us_per_char: u64,
}

impl Out for Host {
    fn put(&mut self, b: &[u8]) -> Result<(), Stalled> {
        self.out.extend_from_slice(b);
        Ok(())
    }
    fn recv(&mut self, wait_ms: u32) -> Option<u8> {
        let b = self.input.pop_front();
        self.us += match b {
            Some(_) => self.us_per_char,
            None => u64::from(wait_ms) * 1_000,
        };
        b
    }
    fn now(&self) -> Ms {
        Ms((self.us / 1_000) as u32)
    }
}

const SEAL: [u8; 8] = *b"SEAL0001";

fn image() -> Vec<u8> {
    let body: Vec<u8> = (0..IMAGE_BODY).map(|i| (i as u8).wrapping_mul(7)).collect();
    let mut c = Crc32::new();
    c.update(&body);
    let h = Header {
        body_len: IMAGE_BODY,
        body_crc: c.finish(),
        door_off: 0x100,
        seal: SEAL,
        version: Version::new("0.2.0 02e440e2"),
    };
    [&h.to_bytes(true)[..], &body].concat()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 48 bytes a line, as the host tool sends.
fn lines(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in b.chunks(48) {
        for c in line.chunks(3) {
            let n = c
                .iter()
                .enumerate()
                .fold(0u32, |n, (i, &x)| n | u32::from(x) << (16 - 8 * i));
            for i in 0..4 {
                out.push(if i <= c.len() {
                    B64[(n >> (18 - 6 * i) & 63) as usize]
                } else {
                    b'='
                });
            }
        }
        out.push(b'\n');
    }
    out
}

#[test]
fn a_1mb_bank2_put_at_16kib_per_s_lands() {
    let img = image();
    assert_eq!(img.len() as u32, BANK2_LEN);
    let mut c = Crc32::new();
    c.update(&img);
    let mut console = Console::new();
    let line = format!(
        "put /CHIMERA/SYSTEM/BANK2.BK2 {} {:08x}\n",
        img.len(),
        c.finish()
    );
    let req = line.bytes().find_map(|b| console.push(b)).unwrap();
    let Ok(Request::Put(arg)) = req else {
        panic!("{req:?}")
    };
    // 16 KiB/s of data: 48 bytes are 65 characters.
    let us_per_char = 48 * 1_000_000 / (16 * 1024 * 65);
    let mut host = Host {
        input: lines(&img).into(),
        out: Vec::new(),
        us: 0,
        us_per_char,
    };
    let mut s = Mem(MemStore::new(1));
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    put(arg, None, Some(Seal::new(SEAL).unwrap()), &mut s, &mut host).unwrap();
    let took = host.now().0;
    assert_eq!(String::from_utf8(host.out).unwrap(), "READY\nOK\n");
    assert!((60_000..PUT_DEADLINE_MS).contains(&took), "{took} ms");
}
