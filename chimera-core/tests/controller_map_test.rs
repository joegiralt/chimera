// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Controller maps (#807, ADR 0226, 0323): one flat list of up to 90 rows,
//! a Part's rows saved into it, and its file on the card (`/CHIMERA/MAPS`,
//! named in the header, its stem from the name); an old sectioned map
//! loads flattened.

mod common;
mod screen;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::ParamId;
use chimera_core::dsp::algo::params::AlgoParams;
use chimera_core::dsp::modal::ModalParams;
use chimera_core::dsp::voice_fx::params::VoiceFxParams;
use chimera_core::midi_map::ctl::{self, ControllerMap};
use chimera_core::midi_map::{Binding, Input, MAP_LEN, MapFull, Mappable, MidiMap};
use chimera_core::name::SoundName;
use chimera_core::params::{EngineType, FilterParams};
use chimera_core::storage::{Card, FileError, LoadError, Ready, SaveError};
use chimera_hal::store::StoreError;
use chimera_hal::store::{Dir, FileName, Store};
use chimera_hal::testkit::MemStore;

static DOOR: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The door open (the host's own under `bank2`), held for the test.
fn open() -> std::sync::MutexGuard<'static, ()> {
    let g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(feature = "bank2")]
    chimera_core::bank2::DOOR_CELL.open_host();
    g
}

fn t(b: BlockRef, p: ParamId) -> Mappable {
    Mappable::new(ParamAddr::new(b, p)).unwrap()
}

fn cc(n: u8, target: Mappable) -> Binding {
    let mut m = MidiMap::EMPTY;
    m.learn(Input::cc(n).unwrap(), target).unwrap();
    m.entries()[0]
}

fn map(rows: &[Binding]) -> MidiMap {
    let mut m = MidiMap::EMPTY;
    for &r in rows {
        m.learn(r.input, r.target).unwrap();
        let n = m.len() - 1;
        m.set(n, r);
    }
    m
}

fn name(s: &str) -> SoundName {
    SoundName::new(s).unwrap()
}

fn cutoff() -> Mappable {
    t(BlockRef::Filter, FilterParams::CUTOFF)
}

fn reso() -> Mappable {
    t(BlockRef::Filter, FilterParams::RESONANCE)
}

fn morph() -> Mappable {
    t(BlockRef::Algo, AlgoParams::MORPH)
}

fn structure() -> Mappable {
    t(BlockRef::Modal, ModalParams::STRUCTURE)
}

fn p1() -> Mappable {
    t(BlockRef::VoiceFx, VoiceFxParams::P1)
}

/// `own` saved into an empty map.
fn saved(own: &[Binding], _e: EngineType) -> ControllerMap {
    let mut m = ControllerMap::EMPTY;
    m.absorb(&map(own)).unwrap();
    m
}

/// Every mappable cell, in table order.
fn cells() -> Vec<Mappable> {
    BlockRef::ALL
        .into_iter()
        .flat_map(|b| (0..64).filter_map(move |p| Mappable::new(ParamAddr::new(b, ParamId(p)))))
        .collect()
}

/// `n` rows on distinct cells and CCs (a CC past 119 wraps onto NRPNs).
fn rows(n: usize) -> ControllerMap {
    let mut m = ControllerMap::EMPTY;
    let mut ccs = (0..=127u8).filter_map(Input::cc);
    for t in cells().into_iter().take(n) {
        let input = ccs
            .next()
            .unwrap_or(Input::Pressure(chimera_midi::take::Takeover::Jump));
        m.push(Binding { input, ..cc(21, t) }).unwrap();
    }
    m
}

/// A map keeps its rows in order, one a target: a row on a target held
/// takes that row's place.
#[test]
fn rows_keep_their_order_one_a_target() {
    let mut m = ControllerMap::EMPTY;
    m.push(cc(20, cutoff())).unwrap();
    m.push(cc(21, morph())).unwrap();
    m.push(cc(22, cutoff())).unwrap();
    assert_eq!(
        m.rows().collect::<Vec<_>>(),
        [cc(22, cutoff()), cc(21, morph())]
    );
}

/// Any engine's cells sit in one list: no sections, no layer.
#[test]
fn every_engine_s_rows_sit_in_one_list() {
    let m = saved(
        &[
            cc(21, cutoff()),
            cc(16, morph()),
            cc(17, structure()),
            cc(18, p1()),
        ],
        EngineType::Algo,
    );
    assert_eq!(m.len(), 4);
    assert_eq!(m.get(2), Some(cc(17, structure())));
}

/// A Part's rows saved into a map clear the rows on their controls or
/// targets, then follow the rest.
#[test]
fn a_part_s_rows_replace_what_they_shadowed() {
    let mut m = saved(
        &[cc(20, cutoff()), cc(21, reso()), cc(22, p1())],
        EngineType::Va,
    );
    m.absorb(&map(&[cc(20, morph()), cc(23, reso())])).unwrap();
    assert_eq!(
        m.rows().collect::<Vec<_>>(),
        [cc(22, p1()), cc(20, morph()), cc(23, reso())]
    );
}

/// Ninety rows fit; the 91st is refused, and a save past 90 leaves the
/// map as it was.
#[test]
fn a_map_holds_ninety_rows() {
    let mut m = rows(ctl::MAP_ROWS);
    assert_eq!(m.len(), 90);
    let before = m;
    let extra = cells()[ctl::MAP_ROWS];
    assert_eq!(m.push(cc(21, extra)), Err(MapFull));
    let own = map(&[Binding {
        input: Input::Pressure(chimera_midi::take::Takeover::Jump),
        ..cc(21, extra)
    }]);
    // Pressure: a control no row is on, a target none holds.
    assert_eq!(m.absorb(&own), Err(MapFull));
    assert_eq!(m, before, "untouched");
}

/// Sixteen SysEx keys a map: a 17th is MAP FULL, a key used twice one.
#[cfg(feature = "midi-map")]
#[test]
fn a_map_holds_sixteen_sysex_keys() {
    let mut m = ControllerMap::EMPTY;
    let ts = cells();
    for (i, &t) in ts.iter().take(ctl::SX_KEYS).enumerate() {
        m.push(sx(i as u32 * 7919 + 1, t)).unwrap();
    }
    assert_eq!(m.push(sx(999_999, ts[40])), Err(MapFull));
    // A key already held takes no slot.
    m.push(sx(1, ts[41])).unwrap();
    assert_eq!(m.get(m.len() - 1), Some(sx(1, ts[41])));
    // A row dropped frees its key (row 1's, held by no other row).
    m.remove(1);
    m.push(sx(999_999, ts[42])).unwrap();
    for i in 0..m.len() {
        assert!(m.get(i).is_some());
    }
}

/// The stem: up to four letters and digits, then the name's CRC in hex.
#[test]
fn the_stem_comes_from_the_name() {
    let (s, n) = ctl::stem(&name("faderfox ec4"));
    assert_eq!(&s[..4], b"FADE");
    assert_eq!(n, 8);
    assert!(s[4..8].iter().all(u8::is_ascii_hexdigit));
    assert_ne!(
        ctl::stem(&name("FADERFOX 2")),
        ctl::stem(&name("FADERFOX 3"))
    );
    let (s, n) = ctl::stem(&name("-"));
    assert_eq!((s[0], n), (b'M', 5));
    assert!(FileName::new(Dir::Maps, &s[..n], b"A").is_some());
}

fn on_card<R>(s: &mut MemStore, f: impl FnOnce(&mut MemStore, &Ready) -> R) -> R {
    let mut card = Card::new();
    let out = card.run(s, |s, r| Ok::<_, StoreError>(f(s, r))).unwrap();
    out.result.unwrap()
}

/// A map saved to the card loads back the same, by its name.
#[test]
fn a_map_round_trips_through_the_card() {
    let _g = open();
    let mut s = MemStore::new(1);
    let m = saved(&[cc(21, cutoff()), cc(16, morph())], EngineType::Algo);
    on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &mut buf(), &m)).unwrap();
    let mut back = ControllerMap::EMPTY;
    let lost = on_card(&mut s, |s, r| {
        ctl::load(s, r, &name("EC4"), &mut buf(), &mut back)
    })
    .unwrap();
    assert_eq!((lost, back), (0, m));
    // Another name: not there.
    let other = on_card(&mut s, |s, r| {
        ctl::load(s, r, &name("LCXL3"), &mut buf(), &mut back)
    });
    assert_eq!(other, Err(LoadError::Missing));
}

/// The listing names every map on the card, in order; no folder is none.
#[test]
fn the_listing_names_the_maps() {
    let _g = open();
    let mut s = MemStore::new(1);
    let mut out = [None; ctl::MAX_MAPS];
    assert_eq!(
        on_card(&mut s, |s, r| ctl::list(s, r, &mut buf(), &mut out)),
        Ok((0, false))
    );
    let m = saved(&[cc(21, cutoff())], EngineType::Va);
    for n in ["OXI E16", "CIRKLON", "EC4"] {
        on_card(&mut s, |s, r| ctl::save(s, r, &name(n), &mut buf(), &m)).unwrap();
    }
    // Saved again: one pair, one row.
    on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &mut buf(), &m)).unwrap();
    assert_eq!(
        on_card(&mut s, |s, r| ctl::list(s, r, &mut buf(), &mut out)),
        Ok((3, false))
    );
    assert_eq!(
        out[..3],
        [
            Some(name("CIRKLON")),
            Some(name("EC4")),
            Some(name("OXI E16"))
        ]
    );
    let vol = s.mount().unwrap();
    let mut files = 0;
    s.list(vol, Dir::Maps, &mut |_, _| files += 1).unwrap();
    assert!(files <= 6, "{files} files: one pair a map");
}

/// A project keeps each Part's controller map by name: saved, loaded.
/// Without `midi-map` (the bench builds) a project holds no pick.
#[cfg(feature = "midi-map")]
#[test]
fn a_project_names_each_part_s_map() {
    use chimera_core::project::PartId;
    let _g = open();
    let s = MemStore::new(1);
    let mut s = s;
    let f = screen::put_with(&mut s, "MAPPED", |p| {
        p.set_controller(PartId::ALL[1], Some(name("FADERFOX")))
    });
    let mut ui = Box::new(screen::stack_ui());
    let mut rig = screen::Rig::new(&mut ui, s);
    rig.load(f);
    let p = rig.ui.project();
    assert_eq!(p.controller(PartId::ALL[0]), None);
    assert_eq!(p.controller(PartId::ALL[1]), Some(name("FADERFOX")));
}

fn buf() -> ctl::FileBuf {
    ctl::FileBuf::new()
}

/// Saves alternate the A and B sides; a side cut short loads the other.
#[test]
fn a_torn_side_loads_the_last_good_map() {
    let _g = open();
    let mut s = MemStore::new(1);
    let one = saved(&[cc(21, cutoff())], EngineType::Algo);
    let two = saved(&[cc(22, cutoff())], EngineType::Algo);
    on_card(&mut s, |s, r| {
        ctl::save(s, r, &name("EC4"), &mut buf(), &one)
    })
    .unwrap();
    on_card(&mut s, |s, r| {
        ctl::save(s, r, &name("EC4"), &mut buf(), &two)
    })
    .unwrap();
    let mut back = ControllerMap::EMPTY;
    on_card(&mut s, |s, r| {
        ctl::load(s, r, &name("EC4"), &mut buf(), &mut back)
    })
    .unwrap();
    assert_eq!(back, two, "the newer side");
    // The newer is B: cut it short.
    let (st, n) = ctl::stem(&name("EC4"));
    s.plant(FileName::new(Dir::Maps, &st[..n], b"B").unwrap(), b"CHIM");
    on_card(&mut s, |s, r| {
        ctl::load(s, r, &name("EC4"), &mut buf(), &mut back)
    })
    .unwrap();
    assert_eq!(back, one, "the last good side");
    // The next save goes over the torn side.
    on_card(&mut s, |s, r| {
        ctl::save(s, r, &name("EC4"), &mut buf(), &two)
    })
    .unwrap();
    on_card(&mut s, |s, r| {
        ctl::load(s, r, &name("EC4"), &mut buf(), &mut back)
    })
    .unwrap();
    assert_eq!(back, two);
}

/// Two names on one stem: the second's save leaves the first's map, and
/// says why.
#[test]
fn a_name_sharing_a_stem_never_overwrites_it() {
    let _g = open();
    let mut s = MemStore::new(1);
    let (a, b) = (name("EC4 AAU"), name("EC4 ACY"));
    assert_eq!(ctl::stem(&a), ctl::stem(&b), "one stem");
    let m = saved(&[cc(21, cutoff())], EngineType::Algo);
    on_card(&mut s, |s, r| ctl::save(s, r, &a, &mut buf(), &m)).unwrap();
    let other = saved(&[cc(22, reso())], EngineType::Algo);
    assert_eq!(
        on_card(&mut s, |s, r| ctl::save(s, r, &b, &mut buf(), &other)),
        Err(SaveError::File(FileError::BadName))
    );
    let mut back = ControllerMap::EMPTY;
    on_card(&mut s, |s, r| ctl::load(s, r, &a, &mut buf(), &mut back)).unwrap();
    assert_eq!(back, m);
}

/// A listing holds `MAX_MAPS`: past that it says the card holds more.
#[test]
fn a_full_listing_says_there_are_more() {
    let _g = open();
    let mut s = MemStore::new(1);
    let mut out = [None; ctl::MAX_MAPS];
    let m = saved(&[cc(21, cutoff())], EngineType::Algo);
    for i in 0..ctl::MAX_MAPS {
        on_card(&mut s, |s, r| {
            ctl::save(s, r, &name(&format!("M{i:02}")), &mut buf(), &m)
        })
        .unwrap();
    }
    let all = on_card(&mut s, |s, r| ctl::list(s, r, &mut buf(), &mut out));
    assert_eq!(all, Ok((ctl::MAX_MAPS, false)));
    on_card(&mut s, |s, r| ctl::save(s, r, &name("M99"), &mut buf(), &m)).unwrap();
    let more = on_card(&mut s, |s, r| ctl::list(s, r, &mut buf(), &mut out));
    assert_eq!(more, Ok((ctl::MAX_MAPS, true)));
}

/// A side whose checksum fails is passed over: the other loads, and with
/// neither good the map is `Missing`.
#[test]
fn a_side_failing_its_checksum_is_passed_over() {
    let _g = open();
    let mut s = MemStore::new(1);
    let one = saved(&[cc(21, cutoff())], EngineType::Algo);
    let two = saved(&[cc(22, cutoff())], EngineType::Algo);
    on_card(&mut s, |s, r| {
        ctl::save(s, r, &name("EC4"), &mut buf(), &one)
    })
    .unwrap();
    on_card(&mut s, |s, r| {
        ctl::save(s, r, &name("EC4"), &mut buf(), &two)
    })
    .unwrap();
    let (st, n) = ctl::stem(&name("EC4"));
    let vol = s.mount().unwrap();
    let flip = |s: &mut MemStore, side: &[u8]| {
        let f = FileName::new(Dir::Maps, &st[..n], side).unwrap();
        let mut b = vec![0u8; 1024];
        let k = chimera_core::storage::read_small(s, vol, f, &mut b).unwrap();
        b.truncate(k);
        b[k / 2] ^= 0x40;
        s.plant(f, &b);
    };
    flip(&mut s, b"B");
    let mut back = ControllerMap::EMPTY;
    on_card(&mut s, |s, r| {
        ctl::load(s, r, &name("EC4"), &mut buf(), &mut back)
    })
    .unwrap();
    assert_eq!(back, one, "the good side");
    flip(&mut s, b"A");
    let gone = on_card(&mut s, |s, r| {
        ctl::load(s, r, &name("EC4"), &mut buf(), &mut back)
    });
    assert_eq!(gone, Err(LoadError::Missing));
    assert_eq!(back, one, "untouched");
}

/// A SysEx row on `t`, its key `k` (#846).
#[cfg(feature = "midi-map")]
fn sx(k: u32, t: Mappable) -> Binding {
    use chimera_core::midi_map::SxKey;
    use chimera_midi::sxmap::Shape;
    let key = SxKey::new(Shape::new(7, false, true).unwrap(), k).unwrap();
    let mut m = MidiMap::EMPTY;
    m.learn(Input::SysEx(key), t).unwrap();
    m.entries()[0]
}

/// Ninety rows, SysEx among them, round-trip through the card.
#[cfg(feature = "midi-map")]
#[test]
fn a_full_map_round_trips_through_the_card() {
    let _g = open();
    let mut s = MemStore::new(1);
    let mut m = rows(ctl::MAP_ROWS - ctl::SX_KEYS);
    let ts = cells();
    for i in 0..ctl::SX_KEYS {
        m.push(sx(i as u32 * 31 + 5, ts[m.len()])).unwrap();
    }
    assert_eq!(m.len(), ctl::MAP_ROWS);
    on_card(&mut s, |s, r| ctl::save(s, r, &name("BOX"), &mut buf(), &m)).unwrap();
    let mut back = ControllerMap::EMPTY;
    let lost = on_card(&mut s, |s, r| {
        ctl::load(s, r, &name("BOX"), &mut buf(), &mut back)
    })
    .unwrap();
    assert_eq!((lost, back), (0, m));
}

/// An old sectioned map (ADR 0226, SUGGEST's) loads flattened: its
/// sections in file order, rows past 90 skipped and counted; its next save
/// writes `MapRows` only.
#[test]
fn an_old_sectioned_map_loads_flattened() {
    use chimera_core::midi_map::codec;
    use chimera_core::storage::{Crc32, FileKind, Generation, Header, RecordBuf, RecordTag};
    let _g = open();
    let mut s = MemStore::new(1);
    let ts = cells();
    // Five full sections, codes 0..4, as 0.3 wrote them.
    let mut file = Header {
        kind: FileKind::Controller,
        generation: Generation::FIRST,
        name: Some(name("OLD")),
    }
    .encode()
    .to_vec();
    let mut want = Vec::new();
    let mut k = 0;
    for code in 0u8..5 {
        let mut sec = MidiMap::EMPTY;
        for _ in 0..MAP_LEN {
            let b = Binding {
                input: Input::cc(20 + (k % 90) as u8).unwrap_or(Input::cc(2).unwrap()),
                ..cc(21, ts[k])
            };
            sec.learn(b.input, b.target).unwrap();
            let n = sec.len() - 1;
            sec.set(n, b);
            want.push(b);
            k += 1;
        }
        let mut p = RecordBuf::new();
        p.u8(code);
        codec::encode(&sec, &mut p);
        file.extend_from_slice(&RecordTag::MapSection.code().to_le_bytes());
        file.extend_from_slice(&(p.as_slice().len() as u16).to_le_bytes());
        file.extend_from_slice(p.as_slice());
    }
    let mut crc = Crc32::new();
    crc.update(&file);
    file.extend_from_slice(&crc.finish().to_le_bytes());
    let (st, n) = ctl::stem(&name("OLD"));
    let f = FileName::new(Dir::Maps, &st[..n], b"A").unwrap();
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    s.make_dir(vol, Dir::Maps).unwrap();
    s.plant(f, &file);
    let mut back = ControllerMap::EMPTY;
    let lost = on_card(&mut s, |s, r| {
        ctl::load(s, r, &name("OLD"), &mut buf(), &mut back)
    })
    .unwrap();
    // Five sections of 16 at most: 80 rows, all kept.
    assert_eq!(lost, 0);
    assert_eq!(back.rows().collect::<Vec<_>>(), want);
    // Saved again: `MapRows` only.
    on_card(&mut s, |s, r| {
        ctl::save(s, r, &name("OLD"), &mut buf(), &back)
    })
    .unwrap();
    let fb = FileName::new(Dir::Maps, &st[..n], b"B").unwrap();
    let mut b = vec![0u8; 2048];
    let k = chimera_core::storage::read_small(&mut s, vol, fb, &mut b).unwrap();
    let b = &b[..k];
    let has = |tag: u16| b.windows(2).any(|w| w == tag.to_le_bytes());
    assert!(has(RecordTag::MapRows.code()));
    assert!(!has(RecordTag::MapSection.code()));
    let mut again = ControllerMap::EMPTY;
    on_card(&mut s, |s, r| {
        ctl::load(s, r, &name("OLD"), &mut buf(), &mut again)
    })
    .unwrap();
    assert_eq!(again, back);
}

/// A map file a newer firmware wrote with a record this one must
/// understand and doesn't is refused, never shown empty.
#[test]
fn an_unknown_critical_record_refuses_the_map() {
    use chimera_core::storage::{Crc32, FileKind, Generation, Header};
    let _g = open();
    let mut s = MemStore::new(1);
    let mut file = Header {
        kind: FileKind::Controller,
        generation: Generation::FIRST,
        name: Some(name("NEW")),
    }
    .encode()
    .to_vec();
    file.extend_from_slice(&0x80FFu16.to_le_bytes());
    file.extend_from_slice(&0u16.to_le_bytes());
    let mut crc = Crc32::new();
    crc.update(&file);
    file.extend_from_slice(&crc.finish().to_le_bytes());
    let (st, n) = ctl::stem(&name("NEW"));
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    s.make_dir(vol, Dir::Maps).unwrap();
    s.plant(FileName::new(Dir::Maps, &st[..n], b"A").unwrap(), &file);
    let mut back = ControllerMap::EMPTY;
    let got = on_card(&mut s, |s, r| {
        ctl::load(s, r, &name("NEW"), &mut buf(), &mut back)
    });
    assert_eq!(got, Err(LoadError::Missing));
}
