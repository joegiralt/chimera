//! The `Bindings` record `0x001F` (spec § 6.4): a version byte, then 8 B an
//! entry `[tag:3 | kind:5] [number hi] [number lo] [block] [param] [min]
//! [max] [flags]`. `0x001E` is read as JUMP without echo and never written;
//! `0x001F` wins over it. Both decode behind `MidiDoor`: closed, a Sound
//! loads with an empty map and saves none. Under `bank2` the door is the
//! host's own (`open_host`), one test at a time.

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
        entry(0, 0, 20).to_vec(),
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
}

#[test]
fn version_2_is_bounds() {
    let _g = open();
    for v in [0, 2, 255] {
        let mut p = bindings(&[entry(0, 0, 20)]);
        p[0] = v;
        assert_eq!(
            load(&file(&[(RecordTag::Bindings, p)])).err(),
            Some(FileError::Bounds),
            "version {v}"
        );
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

/// Closed, neither record is decoded and a save writes none (spec § 6.4).
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
    // Shapes the open door refuses are not even read.
    let bad = load(&file(&[(RecordTag::Bindings, vec![2])]));
    assert!(bad.is_ok());
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
    // `sparse_bound`'s 2 708 B with 0x001E's 4 + 80 B, now 4 + 129 B.
    assert_eq!(
        MAX_SOUND_COST,
        (2708 - (4 + 80) + (4 + 1 + MAP_LEN * 8)).next_multiple_of(16)
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
