//! The `Bindings` record `0x001F` (spec § 6.4): a version byte, then 8 B an
//! entry `[tag:3 | kind:5] [number hi] [number lo] [block] [param] [min]
//! [max] [flags]`. `0x001E` is read as JUMP without echo and never encoded
//! from a map, only written back verbatim as a closed door kept it;
//! `0x001F` wins over it. Both decode behind `MidiDoor`: closed, a Sound
//! loads with an empty map and saves none, and a project keeps each Part's
//! record for its saves (#815). Under `bank2` the door is the host's own
//! (`open_host`), one test at a time.

mod common;

use chimera_core::addr::{BlockRef, ParamAddr};
use chimera_core::block::{DiskCode, ParamId};
use chimera_core::dsp::lfo::LfoParams;
use chimera_core::dsp::modulator::LfoSlot;
use chimera_core::midi_map::codec::{self, Bounds, MapRecord};
use chimera_core::midi_map::{
    Binding, ButtonMode, Cc14Msb, CcKind, Feedback, Input, LearnCc, MAP_LEN, Mappable, MidiMap,
    NrpnKind, Span,
};
use chimera_core::params::{EngineType, FilterParams, OutParams};
use chimera_core::part::PartParams;
use chimera_core::preset::Sound;
use chimera_core::storage::{FileError, MAX_SOUND_COST, RecordBuf, RecordTag, SoundCheck};
use chimera_midi::rel::ByteEncoding;
use chimera_midi::take::Takeover;
use chimera_midi::value::U14;
use common::codec_util::{decode, encode, record_offsets, sound_file};

static DOOR: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The door open (the host's own under `bank2`), held for the test.
fn open() -> std::sync::MutexGuard<'static, ()> {
    let g = DOOR.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(feature = "bank2")]
    chimera_core::bank2::DOOR_CELL.open_host();
    g
}

fn code(b: BlockRef) -> u8 {
    b.disk_code().unwrap()
}

fn target(b: BlockRef, p: ParamId) -> Mappable {
    Mappable::new(ParamAddr::new(b, p)).unwrap()
}

fn bind(input: Input, t: Mappable, min: u8, max: u8, feedback: Feedback) -> Binding {
    Binding {
        input,
        target: t,
        span: Span { min, max },
        feedback,
    }
}

fn map_of(es: &[Binding]) -> MidiMap {
    let mut m = MidiMap::EMPTY;
    for &e in es {
        m.learn(e.input, e.target).unwrap();
        m.set(m.len() - 1, e);
    }
    m
}

fn encoded(m: &MidiMap) -> Vec<u8> {
    let mut r = RecordBuf::new();
    codec::encode(m, &mut r);
    r.as_slice().to_vec()
}

fn cc(n: u8) -> LearnCc {
    LearnCc::new(n).unwrap()
}

fn cutoff() -> Mappable {
    target(BlockRef::Filter, FilterParams::CUTOFF)
}

fn res() -> Mappable {
    target(BlockRef::Filter, FilterParams::RESONANCE)
}

/// An Algo Sound file of `records` after its `Engine`.
fn file(records: &[(RecordTag, Vec<u8>)]) -> Vec<u8> {
    let mut v = vec![(RecordTag::Engine, vec![EngineType::Algo.disk_code()])];
    v.extend_from_slice(records);
    sound_file(&v)
}

/// Both passes, as the card does: the skips counted and the Sound.
fn load(bytes: &[u8]) -> Result<(u16, Sound), FileError> {
    use chimera_core::storage::Framer;
    let mut s = Sound::neutral(EngineType::Algo);
    let mut c = SoundCheck::new();
    let mut f = Framer::new(bytes.len() as u32)?;
    f.push(bytes, &mut |e| c.step(e, None))?;
    f.finish()?;
    c.finish(None)?;
    let mut f = Framer::new(bytes.len() as u32)?;
    f.push(bytes, &mut |e| c.step(e, Some(&mut s)))?;
    f.finish()?;
    c.finish(Some(&mut s))?;
    Ok((c.skips(), s))
}

/// One `0x001F` entry on CUTOFF, full span, echo off.
fn entry(tag_kind: u8, hi: u8, lo: u8) -> [u8; 8] {
    [
        tag_kind,
        hi,
        lo,
        code(BlockRef::Filter),
        FilterParams::CUTOFF.0,
        0,
        255,
        0,
    ]
}

fn bindings(entries: &[[u8; 8]]) -> Vec<u8> {
    let mut p = vec![1];
    for e in entries {
        p.extend_from_slice(e);
    }
    p
}

/// The record tags a file holds, in order.
fn tags(f: &[u8]) -> Vec<u16> {
    let o = record_offsets(f);
    o[..o.len() - 1]
        .iter()
        .map(|&p| u16::from_le_bytes([f[p], f[p + 1]]))
        .collect()
}

/// A map of every control and a span and echo on each.
fn varied() -> MidiMap {
    let part = |p| target(BlockRef::Part, p);
    map_of(&[
        bind(
            Input::Cc(cc(74), CcKind::Absolute(Takeover::Jump)),
            cutoff(),
            0,
            255,
            Feedback::Off,
        ),
        bind(
            Input::Cc(cc(21), CcKind::Relative(ByteEncoding::BinaryOffset)),
            res(),
            0,
            255,
            Feedback::Value,
        ),
        bind(
            Input::Cc14(Cc14Msb::new(2).unwrap(), Takeover::Pickup),
            part(PartParams::LEVEL),
            0,
            255,
            Feedback::Value,
        ),
        bind(
            Input::Nrpn(U14::new(10 << 7 | 3).unwrap(), NrpnKind::IncDec),
            part(PartParams::PAN),
            200,
            10,
            Feedback::Value,
        ),
        bind(
            Input::Pressure(Takeover::Scale),
            target(BlockRef::Out, OutParams::SEND1),
            0,
            128,
            Feedback::Off,
        ),
        bind(
            Input::Cc(cc(30), CcKind::Button(ButtonMode::Toggle)),
            target(BlockRef::Lfo(LfoSlot::Lfo1), LfoParams::DEPTH),
            3,
            9,
            Feedback::Off,
        ),
    ])
}

/// The frozen bytes of `varied`: the disk format (ADR 0045).
fn varied_bytes() -> Vec<u8> {
    let (f, p, o, l) = (
        code(BlockRef::Filter),
        code(BlockRef::Part),
        code(BlockRef::Out),
        code(BlockRef::Lfo(LfoSlot::Lfo1)),
    );
    [
        &[1][..],
        &[0x00, 0, 74, f, FilterParams::CUTOFF.0, 0, 255, 0],
        &[0x04, 0, 21, f, FilterParams::RESONANCE.0, 0, 255, 1],
        &[0x21, 0, 2, p, PartParams::LEVEL.0, 0, 255, 1],
        &[0x47, 10, 3, p, PartParams::PAN.0, 200, 10, 1],
        &[0x62, 0, 0, o, OutParams::SEND1.0, 0, 128, 0],
        &[0x09, 0, 30, l, LfoParams::DEPTH.0, 3, 9, 0],
    ]
    .concat()
}

#[test]
fn round_trip_is_byte_identical() {
    let _g = open();
    let m = varied();
    assert_eq!(encoded(&m), varied_bytes());
    let (lost, s) = load(&file(&[(RecordTag::Bindings, varied_bytes())])).unwrap();
    assert_eq!(lost, 0);
    assert_eq!(s.map, m);
    assert_eq!(encoded(&s.map), varied_bytes());
    // Through a whole Sound: written as 0x001F, never 0x001E.
    let mut snd = Sound::init(EngineType::Algo);
    snd.map = m;
    let f = encode(&snd);
    assert!(tags(&f).contains(&0x001F));
    assert!(!tags(&f).contains(&0x001E));
    assert_eq!(decode(&f).unwrap().map, m);
}

/// The tag/kind byte of every legal `Input`: a frozen table, append only.
/// A new kind that `Input::kinds` yields fails here until it has a code.
#[test]
fn every_kind_has_a_frozen_code() {
    let _g = open();
    let msb = Cc14Msb::new(2).unwrap();
    let n = U14::new(0).unwrap();
    let controls = [
        Input::Cc(cc(20), CcKind::Absolute(Takeover::Jump)),
        Input::Cc14(msb, Takeover::Jump),
        Input::Nrpn(n, NrpnKind::IncDec),
        Input::Pressure(Takeover::Jump),
    ];
    let want: [&[u8]; 4] = [
        &[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x08, 0x09],
        &[0x20, 0x21, 0x22],
        &[0x40, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49],
        &[0x60, 0x61, 0x62],
    ];
    for (c, want) in controls.into_iter().zip(want) {
        let got: Vec<u8> = c
            .kinds()
            .map(|i| encoded(&map_of(&[bind(i, cutoff(), 0, 255, Feedback::Off)]))[1])
            .collect();
        assert_eq!(got, want, "{c:?}");
    }
}

#[test]
fn legacy_001e_loads_as_jump_without_echo() {
    let _g = open();
    let p = vec![74, code(BlockRef::Filter), FilterParams::CUTOFF.0, 7, 9];
    let (lost, s) = load(&file(&[(RecordTag::MidiMap, p)])).unwrap();
    assert_eq!(lost, 0);
    assert_eq!(
        s.map.entries(),
        [bind(
            Input::Cc(cc(74), CcKind::Absolute(Takeover::Jump)),
            cutoff(),
            7,
            9,
            Feedback::Off
        )]
    );
    // Saved again, it is 0x001F.
    let mut back = Sound::init(EngineType::Algo);
    back.map = s.map;
    let t = tags(&encode(&back));
    assert!(t.contains(&0x001F) && !t.contains(&0x001E));
}

#[test]
fn both_records_001f_wins() {
    let _g = open();
    let old = vec![74, code(BlockRef::Filter), FilterParams::CUTOFF.0, 0, 255];
    let new = {
        let mut e = entry(0x00, 0, 21);
        e[4] = FilterParams::RESONANCE.0;
        bindings(&[e])
    };
    let want = [bind(
        Input::Cc(cc(21), CcKind::Absolute(Takeover::Jump)),
        res(),
        0,
        255,
        Feedback::Off,
    )];
    for order in [
        [
            (RecordTag::MidiMap, old.clone()),
            (RecordTag::Bindings, new.clone()),
        ],
        [
            (RecordTag::Bindings, new.clone()),
            (RecordTag::MidiMap, old.clone()),
        ],
    ] {
        let (_, s) = load(&file(&order)).unwrap();
        assert_eq!(s.map.entries(), want, "{:?}", order.map(|o| o.0));
    }
}

#[test]
fn unknown_kind_skipped_and_counted() {
    let _g = open();
    let p = bindings(&[
        entry(0x0A, 0, 20), // kind 10: none yet
        entry(0x1F, 0, 20), // kind 31
        entry(0x80, 0, 20), // tag 4
        entry(0x07, 0, 20), // IncDec on a CC
        entry(0x23, 0, 2),  // relative on a 14-bit pair
        entry(0x68, 0, 0),  // a button on pressure
        entry(0x60, 0, 5),  // pressure with a number
        entry(0x00, 1, 20), // a CC above 127
        entry(0x00, 0, 20),
    ]);
    let (lost, s) = load(&file(&[(RecordTag::Bindings, p)])).unwrap();
    assert_eq!(lost, 8);
    assert_eq!(s.map.len(), 1);
    assert_eq!(
        s.map.entries()[0].input,
        Input::Cc(cc(20), CcKind::Absolute(Takeover::Jump))
    );
}

#[test]
fn bad_length_is_bounds() {
    let _g = open();
    let full = bindings(&[entry(0, 0, 20); MAP_LEN + 1]);
    for p in [
        vec![],
        vec![1, 0],
        bindings(&[entry(0, 0, 20)])[..8].to_vec(),
        full.clone(),
    ] {
        let n = p.len();
        assert_eq!(
            load(&file(&[(RecordTag::Bindings, p.clone())])).err(),
            Some(FileError::Bounds),
            "{n} B"
        );
        assert_eq!(codec::decode(MapRecord::Bindings, &p, None), Err(Bounds));
    }
    // The version alone: no entries, an empty map.
    let (lost, s) = load(&file(&[(RecordTag::Bindings, vec![1])])).unwrap();
    assert_eq!((lost, s.map.len()), (0, 0));
    // 16 entries fit.
    let sixteen = bindings(&[entry(0, 0, 20); MAP_LEN]);
    assert!(load(&file(&[(RecordTag::Bindings, sixteen)])).is_ok());
    // `0x001E`: 5 B entries, at most 16.
    let old = [74, code(BlockRef::Filter), FilterParams::CUTOFF.0, 0, 255];
    for n in [4, 6, 5 * (MAP_LEN + 1)] {
        let p: Vec<u8> = old.iter().copied().cycle().take(n).collect();
        assert_eq!(
            load(&file(&[(RecordTag::MidiMap, p.clone())])).err(),
            Some(FileError::Bounds),
            "{n} B"
        );
        assert_eq!(codec::decode(MapRecord::Legacy, &p, None), Err(Bounds));
    }
}

/// A version this firmware can't read (#816): the Sound loads, its map
/// skipped and counted once, and the record still wins over `0x001E`.
#[test]
fn unknown_version_skipped() {
    let _g = open();
    let old = vec![74, code(BlockRef::Filter), FilterParams::CUTOFF.0, 0, 255];
    for v in [0, 2, 255] {
        for tail in [bindings(&[entry(0, 0, 20)])[1..].to_vec(), vec![9; 3]] {
            let mut p = vec![v];
            p.extend_from_slice(&tail);
            let (lost, s) = load(&file(&[
                (RecordTag::MidiMap, old.clone()),
                (RecordTag::Bindings, p),
            ]))
            .unwrap();
            assert_eq!((lost, s.map.len()), (1, 0), "version {v}");
        }
    }
}

#[test]
fn reserved_cc_skipped() {
    let _g = open();
    let p = bindings(&[
        entry(0x00, 0, 64),
        entry(0x00, 0, 6),
        entry(0x03, 0, 99),
        entry(0x00, 0, 128),
    ]);
    let (lost, s) = load(&file(&[(RecordTag::Bindings, p)])).unwrap();
    assert_eq!(lost, 4);
    assert!(s.map.is_empty());
}

#[test]
fn cc14_msb_6_skipped() {
    let _g = open();
    let p = bindings(&[
        entry(0x20, 0, 6),
        entry(0x20, 0, 0),
        entry(0x20, 0, 32),
        entry(0x20, 1, 2),
    ]);
    let (lost, s) = load(&file(&[(RecordTag::Bindings, p)])).unwrap();
    assert_eq!(lost, 4);
    assert!(s.map.is_empty());
}

#[test]
fn non_mappable_target_skipped() {
    let _g = open();
    let mut channel = entry(0x00, 0, 20);
    (channel[3], channel[4]) = (code(BlockRef::Part), PartParams::CHANNEL.0);
    let mut unknown = entry(0x00, 0, 21);
    (unknown[3], unknown[4]) = (99, 99);
    let p = bindings(&[channel, unknown]);
    let (lost, s) = load(&file(&[(RecordTag::Bindings, p)])).unwrap();
    assert_eq!(lost, 2);
    assert!(s.map.is_empty());
}

#[test]
fn duplicate_target_keeps_the_last() {
    let _g = open();
    let p = bindings(&[entry(0x00, 0, 20), entry(0x01, 0, 21)]);
    let (lost, s) = load(&file(&[(RecordTag::Bindings, p)])).unwrap();
    assert_eq!(lost, 0);
    assert_eq!(
        s.map.entries(),
        [bind(
            Input::Cc(cc(21), CcKind::Absolute(Takeover::Pickup)),
            cutoff(),
            0,
            255,
            Feedback::Off
        )]
    );
}

/// The neutral Sound writes neither map record (ADR 0087): its bytes are
/// `neutral_v1.txt`'s, unchanged.
#[test]
fn neutral_is_no_record() {
    let _g = open();
    assert!(encoded(&MidiMap::EMPTY).is_empty());
    for e in EngineType::ALL {
        let t = tags(&encode(&Sound::neutral(e)));
        assert!(!t.contains(&0x001F) && !t.contains(&0x001E), "{e:?}");
    }
}

/// Closed, neither record is decoded and a Sound save writes none (spec
/// § 6.4): the Part keeps it instead (`closed_door_project_keeps_the_record`).
#[cfg(feature = "bank2")]
#[test]
fn closed_door_loads_empty_map() {
    let _g = open();
    chimera_core::bank2::DOOR_CELL.close();
    let old = vec![74, code(BlockRef::Filter), FilterParams::CUTOFF.0, 0, 255];
    let (lost, s) = load(&file(&[
        (RecordTag::MidiMap, old),
        (RecordTag::Bindings, varied_bytes()),
    ]))
    .unwrap();
    assert_eq!(lost, 0);
    assert!(s.map.is_empty());
    let mut snd = Sound::init(EngineType::Algo);
    snd.map = varied();
    let t = tags(&encode(&snd));
    assert!(!t.contains(&0x001F) && !t.contains(&0x001E));
    // A version this firmware can't read loads; over a full map is
    // refused, as open.
    assert!(load(&file(&[(RecordTag::Bindings, vec![2])])).is_ok());
    let long = bindings(&[entry(0x00, 0, 20); MAP_LEN + 1]);
    assert!(load(&file(&[(RecordTag::Bindings, long)])).is_err());
}

/// Closed, a project save writes each Part's map record back as read, and
/// opened again the map is there (#815). A Part that loads without one
/// keeps none.
#[cfg(feature = "bank2")]
#[test]
fn closed_door_project_keeps_the_record() {
    use chimera_core::project::{PartId, Project};
    use common::codec_util::{decode_project, encode_project};
    let _g = open();
    let a = PartId::ALL[0];
    let (mut p, _) = Project::boxed();
    p.edit_part(a).sound.map = varied();
    let mapped = encode_project(&p);
    p.edit_part(a).sound.map = MidiMap::EMPTY;
    let unmapped = encode_project(&p);

    chimera_core::bank2::DOOR_CELL.close();
    let (mut q, _) = Project::boxed();
    decode_project(&mapped, &mut q).unwrap();
    assert!(q.part(a).sound.map.is_empty());
    let saved = encode_project(&q);
    decode_project(&unmapped, &mut q).unwrap();
    let cleared = encode_project(&q);

    chimera_core::bank2::DOOR_CELL.open_host();
    let (mut r, _) = Project::boxed();
    decode_project(&saved, &mut r).unwrap();
    assert_eq!(r.part(a).sound.map, varied());
    decode_project(&cleared, &mut r).unwrap();
    assert!(r.part(a).sound.map.is_empty());
}

/// A full map is 1 + 16 × 8 B, 49 over `0x001E`'s 80: `MAX_SOUND_COST`
/// carries it (`sound_codec_test::sparse_bound` pins the rest).
#[test]
fn sparse_bound_holds() {
    let _g = open();
    let targets = BlockRef::ALL
        .into_iter()
        .flat_map(|b| b.specs().iter().map(move |s| ParamAddr::new(b, s.id)))
        .filter_map(Mappable::new);
    let mut m = MidiMap::EMPTY;
    for t in targets.take(MAP_LEN) {
        let e = bind(
            Input::Nrpn(U14::new(16383).unwrap(), NrpnKind::IncDec),
            t,
            255,
            0,
            Feedback::Value,
        );
        m.learn(e.input, t).unwrap();
        m.set(m.len() - 1, e);
    }
    assert_eq!(m.len(), MAP_LEN);
    assert_eq!(encoded(&m).len(), 1 + MAP_LEN * 8);
    // `sparse_bound`'s 2 708 B with 0x001E's 4 + 80 B, now 4 + 129 B,
    // and `PerfRoutes`' 4 + 160 B (#514).
    assert_eq!(
        MAX_SOUND_COST,
        (2708 - (4 + 80) + (4 + 1 + MAP_LEN * 8) + (4 + 160)).next_multiple_of(16)
    );
}

/// The decode fuzz target's seeds hold both map records. Written once with
/// `SEED_WRITE=1` (an existing seed is kept), then compared.
#[test]
fn fuzz_seeds_hold_both_records() {
    let _g = open();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/seeds/decode_sound");
    let old = vec![74, code(BlockRef::Filter), FilterParams::CUTOFF.0, 0, 255];
    for (name, bytes) in [
        (
            "bindings.snd",
            file(&[(RecordTag::Bindings, varied_bytes())]),
        ),
        ("legacy_map.snd", file(&[(RecordTag::MidiMap, old.clone())])),
        (
            "both_maps.snd",
            file(&[
                (RecordTag::MidiMap, old),
                (RecordTag::Bindings, varied_bytes()),
            ]),
        ),
    ] {
        let p = dir.join(name);
        if std::env::var_os("SEED_WRITE").is_some() && !p.exists() {
            std::fs::write(&p, &bytes).unwrap();
        }
        let got = std::fs::read(&p).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(got, bytes, "{name}");
        assert!(load(&got).is_ok(), "{name}");
    }
}

/// A Part loaded open stays Clean once the door closes: its slot's map
/// record is what a closed save can't write, not an edit.
#[cfg(feature = "bank2")]
#[test]
fn closed_door_keeps_parts_clean() {
    use chimera_core::project::{PartId, PartStatus, part_status};
    let _g = open();
    let ui = Box::new(chimera_core::ui::UiState::new());
    let p = ui.project();
    let part = p.part(PartId::ALL[0]);
    assert!(!part.sound.map.is_empty());
    assert_eq!(part_status(part, p.pool()), PartStatus::Clean);
    chimera_core::bank2::DOOR_CELL.close();
    assert_eq!(part_status(part, p.pool()), PartStatus::Clean);
}

/// Closed, a Part's load that fails (an empty slot) leaves its Sound and
/// its kept record as they were (#815).
#[cfg(feature = "bank2")]
#[test]
fn closed_door_failed_load_keeps_the_record() {
    use chimera_core::project::{
        PartFrom, PartId, PartSource, Project, ReplaceError, ReplaceGuard,
    };
    use common::codec_util::encode_project;
    let _g = open();
    chimera_core::bank2::DOOR_CELL.close();
    let (mut p, t) = Project::boxed();
    let before = encode_project(&p);
    let empty = p.pool().first_free().unwrap();
    let src = PartSource {
        part: PartId::ALL[0],
        from: PartFrom::Slot(empty),
    };
    let c = ReplaceGuard::check(&p, t, src).unwrap();
    assert_eq!(p.replace_part(c), Err(ReplaceError::SlotEmpty));
    assert_eq!(encode_project(&p), before);
}

/// Closed, a Part saved to a slot keeps its record there: the slot's
/// Sound, read open, has the Part's map (#815).
#[cfg(feature = "bank2")]
#[test]
fn closed_door_part_save_keeps_the_record() {
    use chimera_core::project::test_support::save_part_to;
    use chimera_core::project::{PartId, Project, SlotId};
    let _g = open();
    chimera_core::bank2::DOOR_CELL.close();
    let (mut p, _) = Project::boxed();
    let s = p.pool().first_free().unwrap();
    save_part_to(&mut p, PartId::ALL[0], s);
    chimera_core::bank2::DOOR_CELL.open_host();
    let want = p.pool().get(SlotId::ALL[0]).unwrap().to_sound().map;
    assert!(!want.is_empty());
    assert_eq!(p.pool().get(s).unwrap().to_sound().map, want);
}

/// `f` with each `0x001F` record rewritten as the `0x001E` an old build
/// wrote: 5 B a CC entry `[cc] [block] [param] [min] [max]`.
fn as_legacy(f: &[u8]) -> Vec<u8> {
    let mut out = f[..28].to_vec();
    let o = record_offsets(f);
    for w in o.windows(2) {
        let r = &f[w[0]..w[1]];
        if u16::from_le_bytes([r[0], r[1]]) != 0x001F {
            out.extend_from_slice(r);
            continue;
        }
        let p: Vec<u8> = r[5..]
            .as_chunks::<8>()
            .0
            .iter()
            .filter(|e| e[0] >> 5 == 0)
            .flat_map(|e| [e[2], e[3], e[4], e[5], e[6]])
            .collect();
        out.extend(0x001Eu16.to_le_bytes());
        out.extend((p.len() as u16).to_le_bytes());
        out.extend(p);
    }
    out.extend_from_slice(&f[f.len() - 4..]);
    common::codec_util::fix_crc(&mut out);
    out
}

/// A project saved before `0x001F` loads its Parts Clean (#810): slots and
/// Parts both decode, then re-encode as `0x001F`.
#[test]
fn legacy_project_parts_load_clean() {
    use chimera_core::project::{PartId, PartStatus, Project, part_status};
    use common::codec_util::{decode_project, encode_project};
    let _g = open();
    let (p, _) = Project::boxed();
    let old = as_legacy(&encode_project(&p));
    assert!(tags(&old).contains(&0x001E) && !tags(&old).contains(&0x001F));
    let (mut q, _) = Project::boxed();
    decode_project(&old, &mut q).unwrap();
    for id in PartId::ALL {
        let part = q.part(id);
        assert!(!part.sound.map.is_empty(), "{id:?}");
        assert_eq!(part_status(part, q.pool()), PartStatus::Clean, "{id:?}");
    }
}

/// Closed, a project's slots keep their map records through a load and a
/// save, and a Part loaded from one (REVERT, UPDATE STALE) keeps it too
/// (#862).
#[cfg(feature = "bank2")]
#[test]
fn closed_door_project_slots_keep_the_record() {
    use chimera_core::project::test_support::save_part_to;
    use chimera_core::project::{PartFrom, PartId, PartSource, Project, ReplaceGuard};
    use common::codec_util::{decode_project, encode_project};
    let _g = open();
    let (a, b) = (PartId::ALL[0], PartId::ALL[1]);
    let (mut p, _) = Project::boxed();
    p.edit_part(a).sound.map = varied();
    let s = p.pool().first_free().unwrap();
    save_part_to(&mut p, a, s);
    let file = encode_project(&p);

    chimera_core::bank2::DOOR_CELL.close();
    let (mut q, t) = Project::boxed();
    decode_project(&file, &mut q).unwrap();
    let src = PartSource {
        part: b,
        from: PartFrom::Slot(s),
    };
    q.replace_part(ReplaceGuard::check(&q, t, src).unwrap())
        .unwrap();
    let saved = encode_project(&q);

    chimera_core::bank2::DOOR_CELL.open_host();
    let (mut r, _) = Project::boxed();
    decode_project(&saved, &mut r).unwrap();
    assert_eq!(r.pool().get(s).unwrap().to_sound().map, varied());
    assert_eq!(r.part(b).sound.map, varied());
}

/// Closed, a Part loaded from a slot another Part then saves over reads
/// Stale, not Edited: the slot's map record isn't the Sound's to hash
/// (#862).
#[cfg(feature = "bank2")]
#[test]
fn closed_door_stale_is_stale() {
    use chimera_core::project::test_support::save_part_to;
    use chimera_core::project::{
        PartFrom, PartId, PartSource, PartStatus, Project, ReplaceGuard, SlotId, part_status,
    };
    let _g = open();
    chimera_core::bank2::DOOR_CELL.close();
    let (mut p, t) = Project::boxed();
    let (a, b, s) = (PartId::ALL[2], PartId::ALL[3], SlotId::ALL[1]);
    for part in [a, b] {
        let src = PartSource {
            part,
            from: PartFrom::Slot(s),
        };
        p.replace_part(ReplaceGuard::check(&p, t, src).unwrap())
            .unwrap();
    }
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Clean);
    p.edit_part(b).sound.params.filter.cutoff *= 0.5;
    save_part_to(&mut p, b, s);
    assert_eq!(part_status(p.part(a), p.pool()), PartStatus::Stale(s));
}

/// The library's row for `id` on `st`.
#[cfg(feature = "bank2")]
fn listed(
    st: &mut chimera_hal::testkit::MemStore,
    id: chimera_core::storage::LibId,
) -> chimera_core::project::LibFile {
    use chimera_core::project::{LibPage, PageAt, Shelf, list_library};
    use chimera_core::storage::Card;
    let mut p = Box::new(LibPage::EMPTY);
    list_library(&mut Card::new(), st, Shelf::User, PageAt::First, &mut p)
        .out
        .unwrap();
    let e = p.entries().iter().flatten().find(|e| e.id == id).unwrap();
    p.file(e).unwrap()
}

/// Closed, a Part saved to the library keeps its record, and a library
/// Sound loaded into a Part keeps its own for the project's save (#862).
#[cfg(feature = "bank2")]
#[test]
fn closed_door_library_keeps_the_record() {
    use chimera_core::name::SoundName;
    use chimera_core::project::test_support::scratch_of;
    use chimera_core::project::{
        LibraryPart, PartId, Project, ReplaceGuard, SlotId, read_library_sound, save_library_part,
        save_library_sound,
    };
    use chimera_core::storage::{BuildStamp, Card, LibId};
    use chimera_hal::store::{Dir, Store};
    use chimera_hal::testkit::MemStore;
    use common::codec_util::{decode_project, encode_project};
    const ST: BuildStamp = BuildStamp::new("0.1.0", "02e440e2", false);
    let _g = open();
    let (a, b) = (PartId::ALL[0], PartId::ALL[1]);
    let name = |s: &str| SoundName::new(s).unwrap();
    let mut st = MemStore::new(1);
    let v = st.mount().unwrap();
    st.make_dir(v, Dir::Chimera).unwrap();
    st.make_dir(v, Dir::Sounds).unwrap();
    let mut snd = Sound::init(EngineType::Algo);
    snd.map = varied();
    let mapped: LibId = save_library_sound(&mut Card::new(), &mut st, &snd, name("MAPPED"), &ST)
        .out
        .unwrap()
        .into();

    chimera_core::bank2::DOOR_CELL.close();
    let (mut p, t) = Project::boxed();
    let chart: LibId = save_library_part(&mut Card::new(), &mut st, &p, a, name("CHART"), &ST)
        .out
        .unwrap()
        .into();
    let file = listed(&mut st, mapped);
    read_library_sound(&mut Card::new(), &mut st, &mut p, file)
        .out
        .unwrap();
    let c = ReplaceGuard::check(&p, t, LibraryPart { part: b, file }).unwrap();
    p.adopt(c, None).unwrap();
    let saved = encode_project(&p);

    chimera_core::bank2::DOOR_CELL.open_host();
    let (mut r, _) = Project::boxed();
    decode_project(&saved, &mut r).unwrap();
    assert_eq!(r.part(b).sound.map, varied());
    let want = r.pool().get(SlotId::ALL[0]).unwrap().to_sound().map;
    assert!(!want.is_empty());
    let file = listed(&mut st, chart);
    read_library_sound(&mut Card::new(), &mut st, &mut r, file)
        .out
        .unwrap();
    assert_eq!(scratch_of(&r).map, want);
}

/// Closed, a library Sound equal to a slot's but for its map doesn't
/// dedupe onto that slot: its map would be lost (#862).
#[cfg(feature = "bank2")]
#[test]
fn closed_door_library_load_does_not_dedupe_over_a_map() {
    use chimera_core::factory;
    use chimera_core::project::{
        LibraryPart, PartId, Project, ReplaceGuard, read_library_sound, save_library_sound,
    };
    use chimera_core::storage::{BuildStamp, Card, LibId};
    use chimera_hal::store::{Dir, Store};
    use chimera_hal::testkit::MemStore;
    use common::codec_util::{decode_project, encode_project};
    const ST: BuildStamp = BuildStamp::new("0.1.0", "02e440e2", false);
    let _g = open();
    let b = PartId::ALL[1];
    let mut st = MemStore::new(1);
    let v = st.mount().unwrap();
    st.make_dir(v, Dir::Chimera).unwrap();
    st.make_dir(v, Dir::Sounds).unwrap();
    // NEW's slot 1 Sound, its default chart swapped for another map.
    let mut snd = factory::factory_sound(0).unwrap();
    snd.map = varied();
    let id: LibId = save_library_sound(&mut Card::new(), &mut st, &snd, snd.name, &ST)
        .out
        .unwrap()
        .into();

    chimera_core::bank2::DOOR_CELL.close();
    let (mut p, t) = Project::boxed();
    let file = listed(&mut st, id);
    read_library_sound(&mut Card::new(), &mut st, &mut p, file)
        .out
        .unwrap();
    let c = ReplaceGuard::check(&p, t, LibraryPart { part: b, file }).unwrap();
    p.adopt(c, None).unwrap();
    let saved = encode_project(&p);

    chimera_core::bank2::DOOR_CELL.open_host();
    let (mut r, _) = Project::boxed();
    decode_project(&saved, &mut r).unwrap();
    assert_eq!(r.part(b).sound.map, varied());
}

/// Closed, a project whose slots' map records only just fit loads every
/// slot its plan took: a record keeps only room the plan left over (#862).
#[cfg(feature = "bank2")]
#[test]
fn closed_door_records_never_crowd_out_a_slot() {
    use chimera_core::project::test_support::{full_dense_slots, save_at};
    use chimera_core::project::{
        LoadLink, Project, ProjectSource, ReplaceGuard, SlotId, load_project, project_file,
    };
    use chimera_core::storage::{Card, ProjectId, Side};
    use chimera_hal::store::{ReadSink, Store};
    use chimera_hal::testkit::MemStore;
    struct Bytes(Vec<u8>);
    impl ReadSink for Bytes {
        fn begin(&mut self, _: u32) -> std::ops::ControlFlow<()> {
            std::ops::ControlFlow::Continue(())
        }

        fn chunk(&mut self, b: &[u8]) -> std::ops::ControlFlow<()> {
            self.0.extend_from_slice(b);
            std::ops::ControlFlow::Continue(())
        }
    }
    let _g = open();
    let (mut p, _) = full_dense_slots(SlotId::ALL.len());
    let mut st = MemStore::new(1);
    save_at(&mut Card::new(), &mut st, &mut p, 1);
    let vol = st.mount().unwrap();
    let id = ProjectId::new(1).unwrap();
    let side = project_file(id).side(Side::A);
    let mut f = Bytes(Vec::new());
    st.read(vol, side, &mut f).unwrap();
    let f = f.0;
    // Every map record grown to a full map: more than the arena's slack.
    let mut big = f[..28].to_vec();
    let o = record_offsets(&f);
    for w in o.windows(2) {
        let r = &f[w[0]..w[1]];
        if u16::from_le_bytes([r[0], r[1]]) == 0x001F {
            let p = bindings(&[entry(0x00, 0, 20); MAP_LEN]);
            big.extend(0x001Fu16.to_le_bytes());
            big.extend((p.len() as u16).to_le_bytes());
            big.extend(p);
        } else {
            big.extend_from_slice(r);
        }
    }
    big.extend_from_slice(&f[f.len() - 4..]);
    common::codec_util::fix_crc(&mut big);
    st.write(vol, side, &mut |w| w.put(&big)).unwrap();
    let filled = |p: &Project| {
        SlotId::ALL
            .iter()
            .filter(|&&s| p.pool().get(s).is_some())
            .count()
    };

    chimera_core::bank2::DOOR_CELL.close();
    let (mut q, t) = Project::boxed();
    let go = ReplaceGuard::check(&q, t, ProjectSource::File { id, vol }).unwrap();
    let out = load_project(&mut Card::new(), &mut st, &mut q, go, &LoadLink::new());
    assert!(out.swap.is_some(), "{:?}", out.note);
    assert_eq!(filled(&q), filled(&p));
}
