// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Controller maps over USB (#880, ADR 0226): `put` takes a map's file to
//! `/CHIMERA/MAPS/<stem>.A|B`, `get` reads the side a load takes, `rm`
//! deletes the pair and `ls maps` lists them; the UI reads its layers again.
#![cfg(all(feature = "card-put", feature = "midi-map"))]

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::console::{Console, LsArg, Out, Refusal, Request, Stalled, get, ls, put, rm};
use chimera_core::midi_map::ctl::{self, ControllerMap};
use chimera_core::midi_map::{Input, Mappable, MidiMap, maps_put};
use chimera_core::name::SoundName;
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::storage::{Card, Crc32, LoadError, Ready};
use chimera_hal::Ms;
use chimera_hal::store::{ByteSink, Dir, FileName, Files, ReadSink, Store, StoreError, VolumeId};
use chimera_hal::testkit::MemStore;
use core::ops::ControlFlow;
use std::collections::VecDeque;

/// A card; `full`: every write ends `Full` after its bytes are taken;
/// the files deleted, in order.
struct Mem(MemStore, bool, Vec<FileName>);

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
        if self.1 {
            struct Drop;
            impl ByteSink for Drop {
                fn put(&mut self, _: &[u8]) -> Result<(), StoreError> {
                    Ok(())
                }
            }
            body(&mut Drop)?;
            return Err(StoreError::Full);
        }
        self.0.write(vol, file, body)
    }
    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.2.push(file);
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

impl Host {
    fn new(input: Vec<u8>) -> Host {
        Host {
            input: input.into(),
            out: Vec::new(),
            us: 0,
        }
    }
    fn text(self) -> String {
        String::from_utf8(self.out).unwrap()
    }
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

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in b.chunks(3) {
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
    out
}

fn unbase64(t: &[u8]) -> Vec<u8> {
    let v = |c: u8| B64.iter().position(|&x| x == c).map(|i| i as u32);
    let mut out = Vec::new();
    for q in t.chunks(4) {
        let n = q.iter().fold(0, |n, &c| n << 6 | v(c).unwrap_or(0));
        let pad = q.iter().filter(|&&c| c == b'=').count();
        out.extend_from_slice(&n.to_be_bytes()[1..4 - pad]);
    }
    out
}

/// A console line parsed.
fn parse(line: &str) -> Result<Request, Refusal> {
    let mut c = Console::new();
    let mut got = None;
    for &b in line.as_bytes().iter().chain(b"\n") {
        if let Some(r) = c.push(b) {
            got = Some(r);
        }
    }
    got.unwrap()
}

/// A `put` of `data` to `path`: the unit's answer, or the parser's refusal.
fn ask_put(s: &mut Mem, path: &str, data: &[u8]) -> String {
    ask_put_crc(s, path, data, crc(data))
}

/// `ask_put` with the CRC the host says.
fn ask_put_crc(s: &mut Mem, path: &str, data: &[u8], said: u32) -> String {
    let input = data
        .chunks(48)
        .flat_map(|c| [base64(c), b"\n".to_vec()].concat())
        .collect();
    let mut host = Host::new(input);
    match parse(&format!("put {path} {} {said:08x}", data.len())) {
        Ok(Request::Put(arg)) => put(arg, None, None, s, &mut host).unwrap(),
        other => return format!("{other:?}"),
    }
    host.text()
}

/// Any other request's answer.
fn ask(s: &mut Mem, line: &str) -> String {
    let mut host = Host::new(Vec::new());
    match parse(line) {
        Ok(Request::Get(arg)) => get(arg, s, &mut host).unwrap(),
        Ok(Request::Rm(arg)) => rm(arg, s, &mut host).unwrap(),
        Ok(Request::Ls(arg)) => ls(arg, s, &mut host).unwrap(),
        other => return format!("{other:?}"),
    }
    host.text()
}

fn name(s: &str) -> SoundName {
    SoundName::new(s).unwrap()
}

fn on_card<R>(s: &mut Mem, f: impl FnOnce(&mut Mem, &Ready) -> R) -> R {
    let mut card = Card::new();
    card.run(s, |s, r| Ok::<_, StoreError>(f(s, r)))
        .unwrap()
        .result
        .unwrap()
}

/// A map binding CC `cc` to the cutoff.
fn cutoff_on(cc: u8) -> ControllerMap {
    let a = ParamAddr::new(BlockRef::Filter, FilterParams::CUTOFF);
    let mut own = MidiMap::EMPTY;
    own.learn(Input::cc(cc).unwrap(), Mappable::new(a).unwrap())
        .unwrap();
    let mut m = ControllerMap::EMPTY;
    m.absorb(&own, EngineType::Algo).unwrap();
    m
}

fn side(s: &mut Mem, stem: &str, ext: &[u8]) -> Option<Vec<u8>> {
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
    let f = FileName::new(Dir::Maps, stem.as_bytes(), ext).unwrap();
    s.read(vol, f, &mut k).ok().map(|()| k.0)
}

/// Map `n` as `m`'s file, saved on another card: its stem and bytes.
fn map_file(n: &str, m: &ControllerMap) -> (String, Vec<u8>) {
    let mut other = Mem(MemStore::new(3), false, Vec::new());
    on_card(&mut other, |s, r| ctl::save(s, r, &name(n), m)).unwrap();
    let (st, len) = ctl::stem(&name(n));
    let st = String::from_utf8(st[..len].to_vec()).unwrap();
    let bytes = side(&mut other, &st, b"A").unwrap();
    (st, bytes)
}

/// The unit's stem for these names: the host tool computes the same
/// (`tools/test_chimera_usb.py`).
#[test]
fn the_stem_is_the_tools() {
    let st = |n| {
        let (s, l) = ctl::stem(&name(n));
        String::from_utf8(s[..l].to_vec()).unwrap()
    };
    assert_eq!(st("FADERFOX EC4"), "FADE3C04");
    assert_eq!(st("-"), "MBC34");
}

#[test]
fn a_map_put_loads_and_the_ui_reads_it_again() {
    let mut s = Mem(MemStore::new(7), false, Vec::new());
    let (st, bytes) = map_file("EC4", &cutoff_on(21));
    let was = maps_put();
    let path = format!("/chimera/maps/{}.a", st.to_lowercase());
    assert_eq!(ask_put(&mut s, &path, &bytes), "READY\nOK\n");
    assert_ne!(maps_put(), was, "the layers read again");
    let mut back = ControllerMap::EMPTY;
    on_card(&mut s, |s, r| ctl::load(s, r, &name("EC4"), &mut back)).unwrap();
    assert_eq!(back, cutoff_on(21));
    assert!(on_card(&mut s, |s, r| ctl::list(s, r, &mut [None; 16])).is_ok_and(|(n, _)| n == 1));
}

/// A put writes the side a load does not take, whatever side the host
/// names, and drops the live side only once the new one is whole and
/// judged.
#[test]
fn a_put_writes_the_side_a_load_does_not_take() {
    for named in ["A", "B"] {
        let mut s = Mem(MemStore::new(7), false, Vec::new());
        on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &cutoff_on(30))).unwrap();
        let (st, bytes) = map_file("EC4", &cutoff_on(21));
        assert!(side(&mut s, &st, b"A").is_some() && side(&mut s, &st, b"B").is_none());
        let path = format!("/CHIMERA/MAPS/{st}.{named}");
        assert_eq!(ask_put(&mut s, &path, &bytes), "READY\nOK\n", "{named}");
        assert_eq!(
            side(&mut s, &st, b"A"),
            None,
            "{named}: the old side dropped"
        );
        assert_eq!(side(&mut s, &st, b"B").as_deref(), Some(&bytes[..]));
        let mut back = ControllerMap::EMPTY;
        on_card(&mut s, |s, r| ctl::load(s, r, &name("EC4"), &mut back)).unwrap();
        assert_eq!(back, cutoff_on(21));
    }
}

/// The live map: a newer `.B` over `.A`; its stem and `.B`'s bytes.
fn live_pair() -> (Mem, String, Vec<u8>) {
    let mut s = Mem(MemStore::new(7), false, Vec::new());
    for cc in [30, 31] {
        on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &cutoff_on(cc))).unwrap();
    }
    let (st, _) = map_file("EC4", &cutoff_on(31));
    let b = side(&mut s, &st, b"B").unwrap();
    (s, st, b)
}

/// A put that fails never touches the live side: a CRC the bytes don't
/// have, a full card, a file that is no map. Named either side, only the
/// unkept `.A` is written (and deleted on the failure); the map loads as
/// it was.
#[test]
fn a_failed_put_leaves_the_live_map() {
    let (_, new) = map_file("EC4", &cutoff_on(21));
    let mut sound = new.clone();
    sound[6] = 1;
    for named in ["A", "B"] {
        for why in ["crc", "full", "kind"] {
            let (mut s, st, b) = live_pair();
            let path = format!("/CHIMERA/MAPS/{st}.{named}");
            let said = match why {
                "crc" => ask_put_crc(&mut s, &path, &new, crc(&new) ^ 1),
                "full" => {
                    s.1 = true;
                    let said = ask_put(&mut s, &path, &new);
                    s.1 = false;
                    said
                }
                _ => ask_put(&mut s, &path, &sound),
            };
            assert!(
                said.starts_with("READY\nERR put: "),
                "{why} {named}: {said}"
            );
            assert_eq!(
                side(&mut s, &st, b"B").as_deref(),
                Some(&b[..]),
                "{why} {named}"
            );
            let mut back = ControllerMap::EMPTY;
            on_card(&mut s, |s, r| ctl::load(s, r, &name("EC4"), &mut back)).unwrap();
            assert_eq!(back, cutoff_on(31), "{why} {named}");
        }
    }
}

/// A map's file under a stem its name doesn't make would never load, and
/// could shadow the map that does: refused, nothing left.
#[test]
fn a_map_under_another_name_s_stem_is_refused() {
    let mut s = Mem(MemStore::new(7), false, Vec::new());
    let (_, bytes) = map_file("EC4", &cutoff_on(21));
    let said = ask_put(&mut s, "/CHIMERA/MAPS/LCXL1234.A", &bytes);
    assert!(said.starts_with("READY\nERR put: "), "{said}");
    for ext in [&b"A"[..], b"B"] {
        assert_eq!(side(&mut s, "LCXL1234", ext), None);
    }
}

#[test]
fn get_reads_the_side_a_load_takes_and_rm_deletes_the_pair() {
    let mut s = Mem(MemStore::new(7), false, Vec::new());
    for cc in [30, 31] {
        on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &cutoff_on(cc))).unwrap();
    }
    let (st, _) = map_file("EC4", &cutoff_on(31));
    let newest = side(&mut s, &st, b"B").unwrap();
    let said = ask(&mut s, &format!("get /CHIMERA/MAPS/{st}"));
    let lines: Vec<&str> = said.lines().collect();
    assert_eq!(lines[0], format!("GET {}", newest.len()));
    let body: Vec<u8> = lines[1..lines.len() - 2]
        .iter()
        .flat_map(|l| unbase64(l.as_bytes()))
        .collect();
    assert_eq!(body, newest);
    assert_eq!(lines[lines.len() - 2], format!("CRC {:08x}", crc(&newest)));
    assert_eq!(lines[lines.len() - 1], "OK");
    // `ls maps` names both sides.
    let listed = ask(&mut s, "ls maps");
    assert!(listed.contains(&format!("{st}.A ")), "{listed}");
    assert!(listed.contains(&format!("{st}.B ")), "{listed}");
    assert_eq!(ask(&mut s, &format!("rm /CHIMERA/MAPS/{st}")), "OK\n");
    let mut gone = ControllerMap::EMPTY;
    assert_eq!(
        on_card(&mut s, |s, r| ctl::load(s, r, &name("EC4"), &mut gone)),
        Err(LoadError::Missing)
    );
    assert_eq!(
        ask(&mut s, &format!("rm /CHIMERA/MAPS/{st}")),
        "ERR rm: not found\n"
    );
    assert_eq!(
        ask(&mut s, &format!("get /CHIMERA/MAPS/{st}")),
        "ERR get: not found\n"
    );
}

#[test]
fn only_map_paths_parse() {
    assert_eq!(parse("ls MAPS"), Ok(Request::Ls(LsArg::Maps)));
    for bad in [
        "rm /CHIMERA/MAPS/",
        "rm /CHIMERA/MAPS/FADE3C04.A",
        "rm /CHIMERA/MAPS/../SOUNDS/S0000001",
        "rm /CHIMERA/MAPS/TOOLONGSTEM",
        "rm /CHIMERA/MAPS/FA.E",
        "get /CHIMERA/MAPS/FADE3C04 x",
        "put /CHIMERA/MAPS/FADE3C04 40 0",
        "put /CHIMERA/MAPS/FADE3C04.C 40 0",
        "put /CHIMERA/MAPS/FADE3C04.A 40 0 --over",
        "put /CHIMERA/MAPS/FADE3C04.A 4097 0",
    ] {
        assert!(
            matches!(parse(bad), Err(Refusal::Arguments(_))),
            "{bad}: {:?}",
            parse(bad)
        );
    }
    assert!(parse("rm /chimera/maps/fade3c04").is_ok());
    assert!(parse("put /CHIMERA/MAPS/M.B 4096 0").is_ok());
}

/// `rm` and a map's delete go in ADR 0045's order: the side a load does
/// not take first, so a cut between leaves the live map loading.
#[test]
fn deletes_take_the_unkept_side_first() {
    let order = |s: &Mem| -> Vec<String> {
        s.2.iter()
            .map(|f| String::from_utf8(f.ext().to_vec()).unwrap())
            .collect()
    };
    // `.B` the newer, live side.
    let (mut s, st, _) = live_pair();
    s.2.clear();
    assert_eq!(ask(&mut s, &format!("rm /CHIMERA/MAPS/{st}")), "OK\n");
    assert_eq!(order(&s), ["A", "B"]);
    let (mut s, _, _) = live_pair();
    s.2.clear();
    on_card(&mut s, |s, r| ctl::delete(s, r, &name("EC4"))).unwrap();
    assert_eq!(order(&s), ["A", "B"]);
}
