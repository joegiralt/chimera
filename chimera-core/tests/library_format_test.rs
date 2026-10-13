//! The `Build` record (library spec § Files): the stamp, the stamped
//! encoder, and a `Build` skipped in every file kind.

mod common;

use chimera_core::addr::BlockRef;
use chimera_core::block::DiskCode;
use chimera_core::factory::{FACTORY_LEN, factory_sound};
use chimera_core::mod_path::LABEL_LEN;
use chimera_core::modulation::ModSource;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::Project;
use chimera_core::storage::{
    BUILD_LEN, BuildStamp, Check, Event, FileError, FileKind, Framer, Generation, HEADER_LEN,
    Header, LIB_PEEK, Note, ReadTag, RecordTag, SoundCheck, encode_library_sound, peek_library,
    write_file,
};
use chimera_core::taps::{Jack, TapOp};
use common::codec_util::{VecSink, decode, decode_project_lossy, encode, fix_crc, record_offsets};
use common::project::decode as decode_project;

const ST: BuildStamp = BuildStamp::new("0.1.0", "02e440e2", false);
/// `Build`'s code, as an older build sees it: unknown.
const BUILD: u16 = 0x001C;

fn fixture(name: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v1");
    std::fs::read(p.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn stamped(s: &Sound) -> Vec<u8> {
    written(s, None)
}

fn written(s: &Sound, note: Option<&Note>) -> Vec<u8> {
    let h = Header {
        kind: FileKind::Sound,
        generation: Generation::FIRST,
        name: Some(s.name),
    };
    let mut sink = VecSink(Vec::new());
    write_file(&mut sink, &h, &mut |w| {
        encode_library_sound(s, &ST, note, w)
    })
    .unwrap();
    sink.0
}

fn record(tag: u16, payload: &[u8]) -> Vec<u8> {
    let mut r = tag.to_le_bytes().to_vec();
    r.extend((payload.len() as u16).to_le_bytes());
    r.extend_from_slice(payload);
    r
}

/// `f` with a record inserted at `at`, re-CRC'd.
fn insert_record(f: &[u8], at: usize, tag: u16, payload: &[u8]) -> Vec<u8> {
    let mut f = f.to_vec();
    f.splice(at..at, record(tag, payload));
    fix_crc(&mut f);
    f
}

/// `f` with its `i`th record replaced, re-CRC'd.
fn replace_record(f: &[u8], i: usize, tag: u16, payload: &[u8]) -> Vec<u8> {
    let o = record_offsets(f);
    let mut f = f.to_vec();
    f.splice(o[i]..o[i + 1], record(tag, payload));
    fix_crc(&mut f);
    f
}

/// Where the first record of `tag` ends.
fn record_end(f: &[u8], tag: RecordTag) -> usize {
    let o = record_offsets(f);
    let i = o[..o.len() - 1]
        .iter()
        .position(|&p| u16::from_le_bytes([f[p], f[p + 1]]) == tag.code())
        .expect("a record of that tag");
    o[i + 1]
}

/// Without the default map, so `PLAIN` stays what the encoder wrote then.
fn sounds() -> Vec<Sound> {
    let mut v: Vec<_> = (0..FACTORY_LEN)
        .map(|i| {
            let mut s = factory_sound(i).unwrap();
            s.map = chimera_core::midi_map::MidiMap::EMPTY;
            s
        })
        .collect();
    v.extend([EngineType::Algo, EngineType::Modal, EngineType::Tape].map(Sound::plain_init));
    v
}

#[test]
fn build_tag_is_0x001c_and_not_critical() {
    assert_eq!(RecordTag::Build.code(), 0x001C);
    assert_eq!(RecordTag::Build.code() & 0x8000, 0);
}

#[test]
fn stamp_round_trips_and_prints() {
    let s = BuildStamp::new("0.1.0", "02e440e2", true);
    assert_eq!(BuildStamp::parse(&s.to_bytes()), Some(s));
    assert_eq!(format!("{s}"), "0.1.0 02e440e2+");
    assert_eq!(format!("{ST}"), "0.1.0 02e440e2");
}

#[test]
fn stamp_is_lenient() {
    let mut long = ST.to_bytes().to_vec();
    long.extend_from_slice(&[0xAA; 9]);
    assert_eq!(BuildStamp::parse(&long).unwrap().hash(), "02e440e2");
    assert_eq!(BuildStamp::parse(&long[..3]), None);
    let mut odd = long.clone();
    odd[0] = 0x07;
    assert_eq!(BuildStamp::parse(&odd).unwrap().version(), "?.1.0");
    // Flag bits other than 0 are ignored.
    let mut flags = ST.to_bytes();
    flags[BUILD_LEN - 1] = 0xFE;
    assert_eq!(BuildStamp::parse(&flags), Some(ST));
}

#[test]
fn this_build_needs_a_clean_real_hash() {
    assert!(ST.is_this_build(&ST));
    assert!(!BuildStamp::new("0.1.0", "02e440e2", true).is_this_build(&ST));
    assert!(!ST.is_this_build(&BuildStamp::new("0.1.0", "02e440e2", true)));
    assert!(!ST.is_this_build(&BuildStamp::new("0.1.0", "12e440e2", false)));
    let dev = BuildStamp::new("0.1.0", "dev", false);
    assert!(!dev.is_this_build(&dev));
    let unk = BuildStamp::new("0.1.0", "unknown", false);
    assert!(!unk.is_this_build(&unk));
}

#[test]
#[should_panic]
fn a_long_hash_does_not_fit() {
    let _ = BuildStamp::new("0.1.0", "123456789", false);
}

fn fnv1a(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325, |h, &c| {
        (h ^ u64::from(c)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// Each `sounds()` file's length and FNV-1a, captured from `encode_sound`
/// before the `Build` split (the v1 fixtures predate sparse encoding, so
/// they are not today's bytes): factory, then INIT ALGO, MODAL, TAPE.
const PLAIN: [(usize, u64); 15] = [
    (232, 0xe22c8733f1bd63f2),
    (207, 0x25b03ccc5f0f0232),
    (222, 0x9230ae34f69000cc),
    (197, 0x792b43e5997f6bf6),
    (157, 0x2d008a43ca7e3f7e),
    (162, 0x34123d7b02c3b0b6),
    (268, 0x972b0812999ae0b4),
    (277, 0xdae80c8f09d402aa),
    // CHOIR, TALK BASS, BREATH and VOX, captured when they joined the bank.
    (483, 0xa1b270a0d6ed7e9a),
    (438, 0x35c8685593322b14),
    (446, 0xc543fd314e7a61a9),
    (454, 0x6d802ef9fe3b7b71),
    (132, 0x91e8cb92bf5162e0),
    (132, 0x8575cbc8307a58e3),
    (136, 0x82a8213584981303),
];

#[test]
fn plain_encode_is_unchanged() {
    assert_eq!(sounds().len(), PLAIN.len());
    for (i, s) in sounds().iter().enumerate() {
        let f = encode(s);
        assert_eq!((f.len(), fnv1a(&f)), PLAIN[i], "sound {i}");
    }
}

#[test]
fn a_stamped_sound_decodes_bit_equal() {
    for s in sounds() {
        assert!(decode(&stamped(&s)).unwrap().bits_eq(&s));
    }
}

#[test]
fn a_stamp_sits_second() {
    let f = stamped(&factory_sound(0).unwrap());
    let o = record_offsets(&f);
    assert_eq!(u16::from_le_bytes([f[o[1]], f[o[1] + 1]]), BUILD);
    assert_eq!(&f[o[1] + 4..o[2]], ST.to_bytes());
}

#[test]
fn odd_builds_load() {
    let s = factory_sound(0).unwrap();
    let f = stamped(&s);
    let after_routes = record_end(&f, RecordTag::Routes);
    let repeat = insert_record(&f, after_routes, BUILD, &ST.to_bytes());
    assert!(decode(&repeat).unwrap().bits_eq(&s));
    assert!(
        decode(&replace_record(&f, 1, BUILD, &[0xAA; 30]))
            .unwrap()
            .bits_eq(&s)
    );
    assert!(
        decode(&replace_record(&f, 1, BUILD, &[1, 2, 3]))
            .unwrap()
            .bits_eq(&s)
    );
}

#[test]
fn build_before_engine_is_corrupt() {
    let f = insert_record(
        &stamped(&factory_sound(0).unwrap()),
        HEADER_LEN,
        BUILD,
        &ST.to_bytes(),
    );
    assert_eq!(decode(&f).unwrap_err(), FileError::Corrupt);
}

fn loads(f: &[u8]) -> bool {
    let (mut p, _) = Project::boxed();
    decode_project(f, &mut p).is_ok()
}

#[test]
fn build_in_a_project_slot_loads() {
    let p = fixture("project.prj");
    let at = record_end(&p, RecordTag::Engine);
    assert!(loads(&insert_record(&p, at, BUILD, &ST.to_bytes())));
}

#[test]
fn build_at_project_top_level_loads() {
    let p = fixture("project.prj");
    // Context None, then Fx, then ORBIT's.
    for at in [HEADER_LEN, record_end(&p, RecordTag::Fx)] {
        assert!(loads(&insert_record(&p, at, BUILD, &ST.to_bytes())), "{at}");
    }
    let o = fixture("orbit.prj");
    let at = record_end(&o, RecordTag::Orbit);
    assert!(
        loads(&insert_record(&o, at, BUILD, &ST.to_bytes())),
        "orbit"
    );
}

/// `f` with an entry added to the `Block` record of `code`, making the
/// record (after `Engine`) if there is none.
fn with_block_entry(f: &[u8], code: u8, id: u8, value: f32) -> Vec<u8> {
    with_block_raw(f, code, id, value.to_le_bytes())
}

/// `with_block_entry` with the value's four bytes as given (an enum's u32).
fn with_block_raw(f: &[u8], code: u8, id: u8, raw: [u8; 4]) -> Vec<u8> {
    let mut entry = vec![id];
    entry.extend(raw);
    let o = record_offsets(f);
    let block = (0..o.len() - 1).find(|&i| {
        u16::from_le_bytes([f[o[i]], f[o[i] + 1]]) == RecordTag::Block.code() && f[o[i] + 4] == code
    });
    match block {
        Some(i) => {
            let mut p = f[o[i] + 4..o[i + 1]].to_vec();
            p.extend(entry);
            replace_record(f, i, RecordTag::Block.code(), &p)
        }
        None => {
            let mut p = vec![code];
            p.extend(entry);
            insert_record(
                f,
                record_end(f, RecordTag::Engine),
                RecordTag::Block.code(),
                &p,
            )
        }
    }
}

/// Pass 1 over `bytes` through `c`.
fn feed_pass_one(c: &mut SoundCheck, bytes: &[u8]) {
    let mut f = Framer::new(bytes.len() as u32).unwrap();
    f.push(bytes, &mut |e| c.event(e)).unwrap();
    f.finish().unwrap();
    c.end().unwrap();
}

/// Pass 1, then pass 2 through a `SoundCheck` stepping a staged Sound:
/// the entries lost.
fn decode_skips(bytes: &[u8]) -> u16 {
    let mut c = SoundCheck::new();
    feed_pass_one(&mut c, bytes);
    let mut staged = Sound::neutral(EngineType::Algo);
    let mut f = Framer::new(bytes.len() as u32).unwrap();
    f.push(bytes, &mut |e| c.step(e, Some(&mut staged)))
        .unwrap();
    f.finish().unwrap();
    c.finish(Some(&mut staged)).unwrap();
    c.skips()
}

/// An unknown param, an unknown block, an unknown record tag.
fn three_skip_file() -> Vec<u8> {
    let f = stamped(&Sound::init(EngineType::Algo));
    let f = with_block_entry(&f, BlockRef::Out.disk_code().unwrap(), 250, 1.0);
    let f = with_block_entry(&f, 200, 1, 1.0);
    insert_record(&f, record_end(&f, RecordTag::Routes), 0x0070, &[0; 4])
}

fn all_v1_and_factory() -> Vec<(String, Sound)> {
    let mut v: Vec<_> = sounds()
        .into_iter()
        .enumerate()
        .map(|(i, s)| (format!("sound {i}"), s))
        .collect();
    for n in (0..8).map(|i| format!("factory_{i}.snd")).chain(
        [
            "init_algo.snd",
            "init_modal.snd",
            "init_tape.snd",
            "library_1.snd",
        ]
        .map(String::from),
    ) {
        v.push((n.clone(), decode(&fixture(&n)).unwrap()));
    }
    v
}

#[test]
fn skips_count_what_was_lost() {
    assert_eq!(decode_skips(&three_skip_file()), 3);
}

#[test]
fn every_fixture_and_factory_sound_skips_nothing() {
    for (name, s) in all_v1_and_factory() {
        assert_eq!(decode_skips(&stamped(&s)), 0, "{name}");
    }
}

#[test]
fn translated_v1_modal_skips_nothing() {
    assert_eq!(decode_skips(&fixture("init_modal.snd")), 0);
}

#[test]
fn pass_one_alone_counts_nothing() {
    let mut c = SoundCheck::new();
    feed_pass_one(&mut c, &three_skip_file());
    assert_eq!(c.skips(), 0);
}

#[test]
fn unknown_records_and_enum_codes_count_and_build_does_not() {
    let f = stamped(&Sound::init(EngineType::Algo));
    // Build repeats and odd lengths never count.
    let at = record_end(&f, RecordTag::Routes);
    assert_eq!(decode_skips(&insert_record(&f, at, BUILD, &[1, 2, 3])), 0);
    // An unknown non-critical record after Engine counts one.
    assert_eq!(decode_skips(&insert_record(&f, at, 0x0071, &[9])), 1);
}

/// `f` with `extra` appended to its first record of `tag`.
fn append_to_record(f: &[u8], tag: RecordTag, extra: &[u8]) -> Vec<u8> {
    let o = record_offsets(f);
    let i = (0..o.len() - 1)
        .find(|&i| u16::from_le_bytes([f[o[i]], f[o[i] + 1]]) == tag.code())
        .expect("a record of that tag");
    let mut p = f[o[i] + 4..o[i + 1]].to_vec();
    p.extend_from_slice(extra);
    replace_record(f, i, tag.code(), &p)
}

fn algo() -> Vec<u8> {
    stamped(&Sound::init(EngineType::Algo))
}

#[test]
fn an_unknown_route_source_or_address_counts() {
    let env = ModSource::Env1.disk_code();
    let out = BlockRef::Out.disk_code().unwrap();
    for route in [[0xFF, out, 0, 1], [env, 0xFF, 0xFF, 1]] {
        let f = append_to_record(&algo(), RecordTag::Routes, &route);
        assert_eq!(decode_skips(&f), 1, "{route:?}");
    }
}

#[test]
fn an_unresolvable_registry_or_moddests_entry_counts() {
    let mut reg = vec![0xFF, 0xFF];
    reg.extend([0; LABEL_LEN]);
    assert_eq!(
        decode_skips(&append_to_record(&algo(), RecordTag::Registry, &reg)),
        1
    );
    assert_eq!(
        decode_skips(&append_to_record(
            &algo(),
            RecordTag::ModDests,
            &[0xFF, 0xFF]
        )),
        1
    );
}

#[test]
fn an_unresolvable_tap_route_counts() {
    let tap = [Jack::Tap(TapOp::Op5).disk_code(), 0xFF, 0xFF, 1];
    let f = insert_record(
        &algo(),
        record_end(&algo(), RecordTag::Routes),
        RecordTag::TapRoutes.code(),
        &tap,
    );
    assert_eq!(decode_skips(&f), 1);
}

#[test]
fn a_refused_enum_code_counts_and_a_retired_one_does_not() {
    let tape = stamped(&Sound::init(EngineType::Tape));
    // TAPE's MODEL (34, 0): code 200 is nothing, code 1 is retired REVERSE.
    let bogus = with_block_raw(&tape, 34, 0, 200u32.to_le_bytes());
    assert_eq!(decode_skips(&bogus), 1);
    let retired = with_block_raw(&tape, 34, 0, 1u32.to_le_bytes());
    assert_eq!(decode_skips(&retired), 0);
}

// ---- Note (showcase spec § 1, ADR 0133) ----

const NOTE: u16 = 0x001D;
const TEXT: &str = "PLAY SOFT FOR PAD, HARD FOR BRASS";

fn note(t: &str) -> Note {
    Note::new(t).expect("a valid note")
}

fn peeked(f: &[u8]) -> Option<Note> {
    peek_library(&f[..f.len().min(LIB_PEEK)], f.len() as u32)
        .unwrap()
        .note
}

#[test]
fn note_tag_is_0x001d_and_not_critical() {
    assert_eq!(RecordTag::Note.code(), NOTE);
    assert_eq!(NOTE & 0x8000, 0);
    assert_eq!(LIB_PEEK, 128);
}

#[test]
fn note_round_trips() {
    let s = factory_sound(0).unwrap();
    let f = written(&s, Some(&note(TEXT)));
    assert_eq!(peeked(&f).as_ref().map(Note::as_str), Some(TEXT));
    let plain = decode(&stamped(&s)).unwrap();
    let got = decode(&f).unwrap();
    assert!(got.bits_eq(&plain) && got.bits_eq(&s));
    assert_eq!(peeked(&stamped(&s)), None);
}

#[test]
fn note_sits_third_and_a_full_one_peeks() {
    let s = factory_sound(0).unwrap();
    let max = "A".repeat(40);
    let f = written(&s, Some(&note(&max)));
    let o = record_offsets(&f);
    assert_eq!(u16::from_le_bytes([f[o[2]], f[o[2] + 1]]), NOTE);
    assert_eq!(&f[o[2] + 4..o[3]], max.as_bytes());
    assert_eq!(peeked(&f).unwrap().as_str(), max);
}

#[test]
fn note_41_bytes_ignored() {
    let s = factory_sound(0).unwrap();
    let f = written(&s, Some(&note("X")));
    let f = replace_record(&f, 2, NOTE, &[b'A'; 41]);
    assert_eq!(peeked(&f), None);
    assert!(decode(&f).unwrap().bits_eq(&s));
}

#[test]
fn note_bad_char_rejected_by_new() {
    for bad in ["", "lower", "TAB\t", "HASH #", "CAF\u{c9}", &"A".repeat(41)] {
        assert_eq!(Note::new(bad), None, "{bad:?}");
    }
    for ok in ["A", "AZ09 .,'-+/:()&%", &"A".repeat(40)] {
        assert_eq!(Note::new(ok).unwrap().as_str(), ok);
    }
}

#[test]
fn unprintable_peeked_bytes_read_as_question_marks() {
    let s = factory_sound(0).unwrap();
    let f = replace_record(&written(&s, Some(&note("X"))), 2, NOTE, b"A\x07\xFFb");
    assert_eq!(peeked(&f).unwrap().as_str(), "A??b");
    let empty = replace_record(&f, 2, NOTE, b"");
    assert_eq!(peeked(&empty), None);
}

#[test]
fn second_or_misplaced_note_ignored() {
    let s = factory_sound(0).unwrap();
    // A second one, even straight after the first: the third is read.
    let f = written(&s, Some(&note("FIRST")));
    let second = insert_record(&f, record_offsets(&f)[3], NOTE, b"SECOND");
    assert_eq!(peeked(&second).unwrap().as_str(), "FIRST");
    assert!(decode(&second).unwrap().bits_eq(&s));
    // One after Routes, not third.
    let plain = stamped(&s);
    let late = insert_record(&plain, record_end(&plain, RecordTag::Routes), NOTE, b"LATE");
    assert_eq!(peeked(&late), None);
    assert!(decode(&late).unwrap().bits_eq(&s));
    // One before Build: Build is then fourth.
    let early = insert_record(&plain, record_offsets(&plain)[1], NOTE, b"EARLY");
    assert_eq!(peeked(&early), None);
    assert!(decode(&early).unwrap().bits_eq(&s));
}

#[test]
fn note_before_engine_is_corrupt() {
    let f = insert_record(&stamped(&factory_sound(0).unwrap()), HEADER_LEN, NOTE, b"X");
    assert_eq!(decode(&f).unwrap_err(), FileError::Corrupt);
}

/// Where the first Part's own `Engine` record ends.
fn part_engine_end(f: &[u8]) -> usize {
    let o = record_offsets(f);
    let tag = |i: usize| u16::from_le_bytes([f[o[i]], f[o[i] + 1]]);
    let part = (0..o.len() - 1).find(|&i| tag(i) == RecordTag::Part.code());
    let engine = (part.unwrap()..o.len() - 1).find(|&i| tag(i) == RecordTag::Engine.code());
    o[engine.unwrap() + 1]
}

#[test]
fn note_in_project_contexts_skipped() {
    let p = fixture("project.prj");
    let base = {
        let (mut t, _) = Project::boxed();
        decode_project_lossy(&p, &mut t).unwrap()
    };
    // Slot, Part, Fx, and the top level before them.
    let ats = [
        record_end(&p, RecordTag::Engine),
        part_engine_end(&p),
        record_end(&p, RecordTag::Fx),
        HEADER_LEN,
    ];
    for at in ats {
        let (mut t, _) = Project::boxed();
        let f = insert_record(&p, at, NOTE, b"A NOTE");
        assert_eq!(
            decode_project_lossy(&f, &mut t),
            Ok(base),
            "{at} of {ats:?}"
        );
    }
    let o = fixture("orbit.prj");
    let f = insert_record(&o, record_end(&o, RecordTag::Orbit), NOTE, b"A NOTE");
    assert!(loads(&f), "orbit");
    // Skipped, never counted: a Sound's skip count stays 0 for a Note.
    let s = stamped(&Sound::init(EngineType::Algo));
    let f = insert_record(&s, record_end(&s, RecordTag::Routes), NOTE, b"A NOTE");
    assert_eq!(decode_skips(&f), 0);
}

#[test]
fn old_build_skips_note() {
    // An older build has no 0x001D: it reads it as an unknown non-critical
    // record, which counts one and loads the Sound.
    let s = Sound::init(EngineType::Algo);
    let f = written(&s, Some(&note(TEXT)));
    assert!(!ReadTag::Unknown(NOTE).critical());
    let mut c = SoundCheck::new();
    let mut staged = Sound::neutral(EngineType::Algo);
    let mut fr = Framer::new(f.len() as u32).unwrap();
    fr.push(&f, &mut |e| match e {
        Event::Record(ReadTag::Known(RecordTag::Note), p) => {
            c.step(Event::Record(ReadTag::Unknown(NOTE), p), Some(&mut staged))
        }
        e => c.step(e, Some(&mut staged)),
    })
    .unwrap();
    fr.finish().unwrap();
    c.finish(Some(&mut staged)).unwrap();
    assert_eq!(c.skips(), 1);
    assert!(staged.bits_eq(&s));
}

#[test]
fn note_golden() {
    let f = written(&factory_sound(0).unwrap(), Some(&note("HELLO, WORLD")));
    let at = record_offsets(&f)[2];
    assert_eq!(&f[at..at + 4], [0x1D, 0x00, 12, 0x00]);
    assert_eq!(&f[at + 4..at + 16], b"HELLO, WORLD");
    // Header 28 + Engine 5 + Build 25, then the Note: inside the peek.
    assert_eq!(at, HEADER_LEN + 5 + 25);
    assert!(at + 4 + 40 <= LIB_PEEK);
}
