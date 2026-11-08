//! Controller maps (#807, ADR 0226): a map's sections, the layer a Part on
//! an engine plays, a Part's rows saved into it, and its file on the card
//! (`/CHIMERA/MAPS`, named in the header, its stem from the name).

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

/// `own` saved into an empty map by a Part on `e`.
fn saved(own: &[Binding], e: EngineType) -> ControllerMap {
    let mut m = ControllerMap::EMPTY;
    m.absorb(&map(own), e).unwrap();
    m
}

/// A Part's rows go to their target's section: an engine's own block to
/// that engine's, anything every engine reads to the shared one.
#[test]
fn rows_join_their_target_s_section() {
    let m = saved(&[cc(21, cutoff()), cc(22, morph())], EngineType::Algo);
    assert_eq!(m.section(None).entries(), &[cc(21, cutoff())]);
    assert_eq!(
        m.section(Some(EngineType::Algo)).entries(),
        &[cc(22, morph())]
    );
    assert!(m.section(Some(EngineType::Modal)).is_empty());
}

/// A Part on an engine plays that engine's rows, then the shared rows on
/// other controls: on a clash the engine's row wins.
#[test]
fn the_layer_is_the_engine_s_rows_then_the_shared_ones() {
    let mut m = saved(&[cc(21, cutoff()), cc(16, p1())], EngineType::Algo);
    m.absorb(&map(&[cc(16, structure())]), EngineType::Modal)
        .unwrap();
    let (algo, lost) = m.layer(EngineType::Algo);
    assert_eq!(lost, 0);
    assert_eq!(algo.entries(), &[cc(21, cutoff()), cc(16, p1())]);
    let (modal, _) = m.layer(EngineType::Modal);
    assert_eq!(modal.entries(), &[cc(16, structure()), cc(21, cutoff())]);
}

/// Saving a Part's rows over a map replaces the rows they shadowed in the
/// Part's layer (same control or same target); other engines' rows stay.
#[test]
fn a_part_s_rows_replace_what_they_shadowed() {
    let mut m = saved(&[cc(21, cutoff()), cc(22, reso())], EngineType::Algo);
    m.absorb(&map(&[cc(16, structure())]), EngineType::Modal)
        .unwrap();
    // CC 21 now moves the resonance, CC 30 the cutoff.
    m.absorb(&map(&[cc(21, reso()), cc(30, cutoff())]), EngineType::Algo)
        .unwrap();
    assert_eq!(
        m.section(None).entries(),
        &[cc(21, reso()), cc(30, cutoff())]
    );
    assert_eq!(
        m.section(Some(EngineType::Modal)).entries(),
        &[cc(16, structure())]
    );
}

/// A shared row on a control the engine's section holds shadows it in the
/// layer, so a save drops the engine's row: the layer plays what the Part
/// played.
#[test]
fn a_shared_row_saved_over_an_engine_row_wins_it() {
    let mut m = saved(&[cc(16, structure())], EngineType::Modal);
    m.absorb(&map(&[cc(16, p1())]), EngineType::Modal).unwrap();
    assert!(m.section(Some(EngineType::Modal)).is_empty());
    assert_eq!(m.layer(EngineType::Modal).0.entries(), &[cc(16, p1())]);
}

/// Seventeen targets every engine reads.
fn shared_targets() -> Vec<Mappable> {
    [BlockRef::Filter, BlockRef::Out, BlockRef::Part]
        .into_iter()
        .flat_map(|b| (0..64).filter_map(move |p| Mappable::new(ParamAddr::new(b, ParamId(p)))))
        .filter(|m| m.engine().is_none())
        .take(MAP_LEN + 1)
        .collect()
}

/// A save that would put more than `MAP_LEN` rows in a layer is refused,
/// the map as it was.
#[test]
fn a_full_layer_refuses_the_save() {
    let ts = shared_targets();
    assert_eq!(ts.len(), MAP_LEN + 1);
    let free = (20..120).filter(|&n| Input::cc(n).is_some());
    let rows: Vec<_> = ts.iter().zip(free).map(|(&t, n)| cc(n, t)).collect();
    let mut m = saved(&rows[..MAP_LEN - 1], EngineType::Algo);
    // ALGO's layer: 15 shared + MORPH = 16.
    m.absorb(&map(&[cc(90, morph())]), EngineType::Algo)
        .unwrap();
    let before = m;
    assert_eq!(
        m.absorb(&map(&[rows[MAP_LEN - 1]]), EngineType::Va),
        Err(MapFull)
    );
    assert_eq!(m, before);
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
    on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &m)).unwrap();
    let mut back = ControllerMap::EMPTY;
    let lost = on_card(&mut s, |s, r| ctl::load(s, r, &name("EC4"), &mut back)).unwrap();
    assert_eq!((lost, back), (0, m));
    // Another name: not there.
    let other = on_card(&mut s, |s, r| ctl::load(s, r, &name("LCXL3"), &mut back));
    assert_eq!(other, Err(LoadError::Missing));
}

/// The listing names every map on the card, in order; no folder is none.
#[test]
fn the_listing_names_the_maps() {
    let _g = open();
    let mut s = MemStore::new(1);
    let mut out = [None; ctl::MAX_MAPS];
    assert_eq!(
        on_card(&mut s, |s, r| ctl::list(s, r, &mut out)),
        Ok((0, false))
    );
    let m = saved(&[cc(21, cutoff())], EngineType::Va);
    for n in ["OXI E16", "CIRKLON", "EC4"] {
        on_card(&mut s, |s, r| ctl::save(s, r, &name(n), &m)).unwrap();
    }
    // Saved again: one pair, one row.
    on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &m)).unwrap();
    assert_eq!(
        on_card(&mut s, |s, r| ctl::list(s, r, &mut out)),
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

/// Saves alternate the A and B sides; a side cut short loads the other.
#[test]
fn a_torn_side_loads_the_last_good_map() {
    let _g = open();
    let mut s = MemStore::new(1);
    let one = saved(&[cc(21, cutoff())], EngineType::Algo);
    let two = saved(&[cc(22, cutoff())], EngineType::Algo);
    on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &one)).unwrap();
    on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &two)).unwrap();
    let mut back = ControllerMap::EMPTY;
    on_card(&mut s, |s, r| ctl::load(s, r, &name("EC4"), &mut back)).unwrap();
    assert_eq!(back, two, "the newer side");
    // The newer is B: cut it short.
    let (st, n) = ctl::stem(&name("EC4"));
    s.plant(FileName::new(Dir::Maps, &st[..n], b"B").unwrap(), b"CHIM");
    on_card(&mut s, |s, r| ctl::load(s, r, &name("EC4"), &mut back)).unwrap();
    assert_eq!(back, one, "the last good side");
    // The next save goes over the torn side.
    on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &two)).unwrap();
    on_card(&mut s, |s, r| ctl::load(s, r, &name("EC4"), &mut back)).unwrap();
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
    on_card(&mut s, |s, r| ctl::save(s, r, &a, &m)).unwrap();
    let other = saved(&[cc(22, reso())], EngineType::Algo);
    assert_eq!(
        on_card(&mut s, |s, r| ctl::save(s, r, &b, &other)),
        Err(SaveError::File(FileError::BadName))
    );
    let mut back = ControllerMap::EMPTY;
    on_card(&mut s, |s, r| ctl::load(s, r, &a, &mut back)).unwrap();
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
            ctl::save(s, r, &name(&format!("M{i:02}")), &m)
        })
        .unwrap();
    }
    let all = on_card(&mut s, |s, r| ctl::list(s, r, &mut out));
    assert_eq!(all, Ok((ctl::MAX_MAPS, false)));
    on_card(&mut s, |s, r| ctl::save(s, r, &name("M99"), &m)).unwrap();
    let more = on_card(&mut s, |s, r| ctl::list(s, r, &mut out));
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
    on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &one)).unwrap();
    on_card(&mut s, |s, r| ctl::save(s, r, &name("EC4"), &two)).unwrap();
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
    on_card(&mut s, |s, r| ctl::load(s, r, &name("EC4"), &mut back)).unwrap();
    assert_eq!(back, one, "the good side");
    flip(&mut s, b"A");
    let gone = on_card(&mut s, |s, r| ctl::load(s, r, &name("EC4"), &mut back));
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

/// Up to `n` SysEx rows: each engine's section in turn, full.
#[cfg(feature = "midi-map")]
fn sysex_map(n: usize) -> ControllerMap {
    let mut m = ControllerMap::EMPTY;
    let mut k = 0;
    let mut rows = |m: &mut ControllerMap, e: Option<EngineType>, on: EngineType| {
        let ts: Vec<_> = BlockRef::ALL
            .into_iter()
            .flat_map(|b| (0..64).filter_map(move |p| Mappable::new(ParamAddr::new(b, ParamId(p)))))
            .filter(|t| t.engine() == e)
            .take(MAP_LEN)
            .collect();
        let mut own = Vec::new();
        for t in ts {
            if k == n {
                break;
            }
            k += 1;
            own.push(sx(k as u32 * 7919, t));
        }
        m.absorb(&map(&own), on).unwrap();
    };
    // Each engine's section full (a layer holds at most `MAP_LEN`: the
    // shared section stays empty).
    for e in EngineType::ALL {
        rows(&mut m, Some(e), e);
    }
    m
}

/// A map whose SysEx rows don't fit its file is refused, the card's map
/// as it was; the largest that fits saves and loads back whole.
#[cfg(feature = "midi-map")]
#[test]
fn a_sysex_map_past_the_file_is_refused_and_the_largest_round_trips() {
    let _g = open();
    let mut s = MemStore::new(1);
    let first = saved(&[cc(21, cutoff())], EngineType::Algo);
    on_card(&mut s, |s, r| ctl::save(s, r, &name("BOX"), &first)).unwrap();
    let full = sysex_map(usize::MAX);
    assert!(!ctl::fits(&full));
    let got = on_card(&mut s, |s, r| ctl::save(s, r, &name("BOX"), &full));
    assert_eq!(got, Err(SaveError::File(FileError::Bounds)));
    let mut back = ControllerMap::EMPTY;
    on_card(&mut s, |s, r| ctl::load(s, r, &name("BOX"), &mut back)).unwrap();
    assert_eq!(back, first, "the card's map untouched");
    // The largest that fits.
    let n = (1..200)
        .take_while(|&n| ctl::fits(&sysex_map(n)))
        .last()
        .unwrap();
    let big = sysex_map(n);
    on_card(&mut s, |s, r| ctl::save(s, r, &name("BOX"), &big)).unwrap();
    let mut back = ControllerMap::EMPTY;
    let lost = on_card(&mut s, |s, r| ctl::load(s, r, &name("BOX"), &mut back)).unwrap();
    assert_eq!((lost, back), (0, big), "{n} rows");
}
