//! TAPE's source format on the card (TAPE Task 10, spec § 1 and § 8): a
//! `ZoneSet`'s zones, its CRC and `held`, the descriptor `held` starts
//! with, `SourceCheck` for `put`, and a Sound's `ZoneTune`. The head, the
//! plain CRC and the writer are the pool's (`source_codec_test`).

mod common;

use chimera_core::dsp::tape_engine::read::{
    DESC_LEN, Descriptor, Shape, SourceView, ZONE_LAYOUT_LEN,
};
use chimera_core::name::SoundName;
use chimera_core::params::EngineType;
use chimera_core::preset::Sound;
use chimera_core::project::arena::Cost;
use chimera_core::project::{Project, SourceId, SourceRef};
use chimera_core::storage::{
    Card, FileError, FileKind, Generation, Header, LoadError, PutRefusal, RecordTag, SaveError,
    SourceCheck, SourceHead, SourceLayout, SourceZones, Wrote, Zone, ZoneTune, held_for_file_len,
    held_len, read_source_into, source_crc, source_file_min_len, write_descriptor, write_file,
    write_source, write_zone_set, zone_set_crc,
};
use chimera_hal::store::{Dir, Store};
use chimera_hal::testkit::MemStore;
use common::codec_util::{VecSink, decode, encode};

/// `n` packed bytes, a pattern of `seed`.
fn packed(seed: u8, frames: u32) -> Vec<u8> {
    (0..3 * frames.div_ceil(2) as usize)
        .map(|i| (i as u32).wrapping_mul(29).wrapping_add(seed as u32) as u8)
        .collect()
}

/// `n` zones of `frames` each, end to end, keys a major third apart from C1,
/// each with its own root and period.
fn zones_of(n: usize, frames: u32) -> Vec<Zone> {
    (0..n as u32)
        .map(|i| Zone {
            start: i * frames,
            frames,
            key: 24 + 4 * i as u8,
            root: 3_600 + 700 * i as u16,
            period_q16: (100 + i) << 16,
        })
        .collect()
}

/// A `ZoneSet` of `zones` over its samples, its CRC the zone set's.
fn zone_set(seed: u8, zones: &[Zone]) -> (SourceHead, SourceZones, Vec<u8>) {
    let z = SourceZones::new(zones).expect("a valid zone table");
    let data = packed(seed, z.frames());
    let head = SourceHead {
        sref: SourceRef {
            crc: zone_set_crc(&z, &data),
            frames: z.frames(),
            rate: 24_000,
        },
        shape: Shape::ZoneSet,
        count: zones.len() as u8,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new("TRON STR").unwrap(),
    };
    (head, z, data)
}

/// A `Plain` or `Transwave` source of `frames`.
fn whole(shape: Shape, count: u8, frames: u32) -> (SourceHead, Vec<u8>) {
    let data = packed(count, frames);
    let head = SourceHead {
        sref: SourceRef {
            crc: source_crc(shape, count, &data),
            frames,
            rate: 48_000,
        },
        shape,
        count,
        root: 6_000,
        period_q16: 0,
        name: SoundName::new("WHOLE").unwrap(),
    };
    (head, data)
}

/// The keys must stay rising: zone `i`'s key, from `zones_of`.
#[test]
fn zones_of_is_a_valid_table() {
    for n in 1..=24 {
        assert!(SourceZones::new(&zones_of(n, 64)).is_some(), "{n}");
    }
}

/// A zone's root and period are labels: the CRC never sees them. Its
/// start, frames and key are the layout: the CRC covers each.
#[test]
fn crc_ignores_labels() {
    let base = zones_of(12, 1_024);
    let (h, z, data) = zone_set(1, &base);
    let crc = |zs: &[Zone]| zone_set_crc(&SourceZones::new(zs).unwrap(), &data);
    assert_eq!(crc(&base), h.sref.crc);

    let mut labels = base.clone();
    labels[3].root ^= 0x55;
    labels[7].period_q16 = 0;
    assert_eq!(crc(&labels), h.sref.crc, "root and period");

    // Zone 3 ends 2 frames later: its frames and zone 4's start move.
    let mut moved = base.clone();
    moved[3].frames += 2;
    moved[4].start += 2;
    moved[4].frames -= 2;
    assert_ne!(crc(&moved), h.sref.crc, "start and frames");
    let mut key = base.clone();
    key[5].key += 1;
    assert_ne!(crc(&key), h.sref.crc, "key");

    // The pool's CRC left the layout out: a zone set's isn't it.
    assert_ne!(source_crc(Shape::ZoneSet, 12, &data), h.sref.crc);
    assert_eq!(SourceLayout::zoned(h, z).unwrap().crc(&data), h.sref.crc);
}

/// `Plain` and `Transwave` keep the pool's CRC: a layout of none.
#[test]
fn plain_crc_is_pools() {
    for (shape, count) in [(Shape::Plain, 1), (Shape::Transwave, 16)] {
        let (h, data) = whole(shape, count, 16 * 1_216);
        let l = SourceLayout::whole(h).expect("playable");
        assert_eq!(l.crc(&data), source_crc(shape, count, &data), "{shape:?}");
        assert_eq!(l.zones(), &[]);
        assert_eq!(l.held_len(), held_len(h.sref).unwrap(), "{shape:?}");
    }
}

/// The same audio and layout, every label moved (the head's root and a
/// zone's): the same file, `Present`, and one file on the card.
#[test]
fn same_audio_new_root_is_present() {
    let (h, z, data) = zone_set(2, &zones_of(4, 512));
    let mut s = MemStore::new(1);
    let mut card = Card::new();
    let mut write = |s: &mut MemStore, h: &SourceHead, z: &SourceZones| {
        card.run(s, |s, r| write_zone_set(s, r, h, z, &data))
            .unwrap()
            .result
    };
    assert_eq!(write(&mut s, &h, &z), Ok(Wrote::New));
    let mut relabelled = zones_of(4, 512);
    relabelled[2].root = 7_250;
    relabelled[0].period_q16 = 0;
    let moved = SourceHead { root: 4_800, ..h };
    let z2 = SourceZones::new(&relabelled).unwrap();
    assert_eq!(write(&mut s, &moved, &z2), Ok(Wrote::Present));
    let vol = s.mount().unwrap();
    let mut files = 0;
    s.list(vol, Dir::Tapes, &mut |_, _| files += 1).unwrap();
    assert_eq!(files, 1);
}

fn read_back(
    s: &mut MemStore,
    sref: SourceRef,
    room: usize,
) -> (Result<usize, LoadError>, Vec<u8>) {
    let mut out = vec![0xEE; room];
    let r = Card::new()
        .run(s, |s, r| read_source_into(s, r, sref, &mut out))
        .unwrap()
        .result;
    (r, out)
}

/// `read_source_into` leads `held` with the descriptor, a zone set's
/// layout after it, then the samples: what `SourceView` reads.
#[test]
fn descriptor_leads_held() {
    let (h, z, data) = zone_set(3, &zones_of(5, 300));
    let mut s = MemStore::new(1);
    let w = Card::new().run(&mut s, |s, r| write_zone_set(s, r, &h, &z, &data));
    assert_eq!(w.unwrap().result, Ok(Wrote::New));
    let l = SourceLayout::zoned(h, z).unwrap();
    let (r, out) = read_back(&mut s, h.sref, l.held_len() + 7);
    assert_eq!(r, Ok(DESC_LEN + 5 * ZONE_LAYOUT_LEN + data.len()));
    let d = Descriptor::from_bytes(&out[..DESC_LEN]).expect("a descriptor");
    assert_eq!(
        (d.shape, d.count, d.rate, d.head_frames),
        (Shape::ZoneSet, 5, 24_000, 0)
    );
    for (i, zone) in z.zones().iter().enumerate() {
        let b = &out[DESC_LEN + i * ZONE_LAYOUT_LEN..][..ZONE_LAYOUT_LEN];
        assert_eq!(&b[..4], &zone.start.to_le_bytes(), "zone {i}");
        assert_eq!(&b[4..8], &zone.frames.to_le_bytes(), "zone {i}");
        assert_eq!(b[8], zone.key, "zone {i}");
    }
    assert_eq!(&out[l.data_start()..l.held_len()], &data[..]);
    let mut built = vec![0; l.held_len()];
    assert_eq!(write_descriptor(&l, &mut built), l.data_start());
    assert_eq!(&built[..l.data_start()], &out[..l.data_start()]);
    let v = SourceView::new(&out[..l.held_len()]).expect("a view");
    assert_eq!(v.frames(), h.sref.frames);

    // Exactly `held`: room enough.
    let (r, exact) = read_back(&mut s, h.sref, l.held_len());
    assert_eq!(r, Ok(l.held_len()));
    assert_eq!(exact, out[..l.held_len()]);
    // One byte short of the layout's `held`: refused, nothing to trust.
    let (r, _) = read_back(&mut s, h.sref, l.held_len() - 1);
    assert_eq!(r, Err(LoadError::File(FileError::Bounds)));
}

/// What TAPE Task 2's pool refused a `ZoneSet` for: a file sealed under a
/// CRC and a `held` that leave the layout out. Now a file whose CRC omits
/// its layout reads as damaged, a zone table moved under the old stem
/// too, and `held` counts the layout.
#[test]
fn a_layout_outside_the_crc_reads_missing() {
    let zones = zones_of(4, 256);
    let (h, z, data) = zone_set(4, &zones);
    let mut moved = zones.clone();
    moved[1].frames += 2;
    moved[2].start += 2;
    moved[2].frames -= 2;
    let moved = SourceZones::new(&moved).unwrap();
    let no_layout = SourceHead {
        sref: SourceRef {
            crc: source_crc(Shape::ZoneSet, 4, &data),
            ..h.sref
        },
        ..h
    };
    for (what, head, zs) in [("pool's CRC", no_layout, z), ("zones moved", h, moved)] {
        let mut s = MemStore::new(1);
        put_raw(&mut s, &head, Some(&zs), &data);
        let (r, _) = read_back(&mut s, head.sref, 1 << 16);
        assert_eq!(r, Err(LoadError::File(FileError::Corrupt)), "{what}");
    }
    let mut s = MemStore::new(1);
    put_raw(&mut s, &h, Some(&z), &data);
    let (r, _) = read_back(&mut s, h.sref, 1 << 16);
    assert_eq!(r, Ok(held_len(h.sref).unwrap() + 4 * ZONE_LAYOUT_LEN));
    // A zone set's head alone has no layout: the pool's writer refuses it.
    let w = Card::new().run(&mut MemStore::new(1), |s, r| write_source(s, r, &h, &data));
    assert_eq!(w.unwrap().result, Err(SaveError::File(FileError::Bounds)));
}

/// A zone table is checked when it is made: 1 to 24 zones, end to end
/// from 0, each a playable source, keys rising; a head takes only the
/// zones it counts, over its frames.
#[test]
fn zones_are_checked() {
    let ok = zones_of(3, 100);
    assert!(SourceZones::new(&ok).is_some());
    let bad = |f: &dyn Fn(&mut Vec<Zone>)| {
        let mut z = ok.clone();
        f(&mut z);
        SourceZones::new(&z)
    };
    assert_eq!(SourceZones::new(&[]), None, "none");
    assert_eq!(SourceZones::new(&zones_of(25, 64)), None, "25");
    assert_eq!(bad(&|z| z[0].start = 1), None, "not from 0");
    assert_eq!(bad(&|z| z[1].start += 1), None, "a gap");
    assert_eq!(bad(&|z| z[2].start += 1), None, "a gap before the last");
    assert_eq!(bad(&|z| z[1].start -= 1), None, "an overlap");
    assert_eq!(
        bad(&|z| { z[2].frames = 63 }),
        None,
        "a zone under 64 frames"
    );
    assert_eq!(bad(&|z| z[1].key = z[0].key), None, "keys not rising");
    assert_eq!(bad(&|z| z[2].key = 128), None, "not a MIDI note");
    assert!(bad(&|z| z[2].key = 127).is_some(), "G9 is a note");
    assert!(bad(&|z| z[2].frames = 64).is_some(), "64 frames is a zone");
    assert_eq!(bad(&|z| z[2].frames = u32::MAX), None, "past u32");

    // The record: whole zones, at most 24.
    let rec = SourceZones::new(&ok).unwrap().to_record();
    assert!(SourceZones::from_record(rec.as_slice()).is_some());
    let mut long = rec.as_slice().to_vec();
    long.push(0);
    assert_eq!(SourceZones::from_record(&long), None, "a byte past a zone");
    let all = SourceZones::new(&zones_of(24, 64)).unwrap().to_record();
    let mut extra = all.as_slice().to_vec();
    extra.extend_from_slice(&all.as_slice()[..15]);
    assert_eq!(SourceZones::from_record(&extra), None, "25 zones");

    let (h, z, _) = zone_set(5, &ok);
    assert!(SourceLayout::zoned(h, z).is_some());
    let other = |h2: SourceHead| SourceLayout::zoned(h2, z);
    assert_eq!(other(SourceHead { count: 2, ..h }), None, "count");
    let short = SourceRef {
        frames: 299,
        ..h.sref
    };
    assert_eq!(other(SourceHead { sref: short, ..h }), None, "frames");
    assert_eq!(
        other(SourceHead {
            shape: Shape::Transwave,
            ..h
        }),
        None,
        "shape"
    );
    assert_eq!(SourceLayout::whole(h), None, "a ZoneSet as whole");
    let slow = SourceRef {
        rate: 3_999,
        ..h.sref
    };
    assert_eq!(other(SourceHead { sref: slow, ..h }), None, "rate");
}

/// The source records framed whole, unchecked by any writer.
fn file_bytes(
    kind: FileKind,
    head: &SourceHead,
    zones: Option<&SourceZones>,
    data: &[u8],
) -> Vec<u8> {
    let h = Header {
        kind,
        generation: Generation::FIRST,
        name: None,
    };
    let mut v = VecSink(Vec::new());
    write_file(&mut v, &h, &mut |w| {
        w.put(RecordTag::SourceHead, &head.to_bytes())?;
        if let Some(z) = zones {
            w.put(RecordTag::SourceZones, z.to_record().as_slice())?;
        }
        data.chunks(504)
            .try_for_each(|c| w.put(RecordTag::SourceData, c))
    })
    .unwrap();
    v.0
}

fn put_raw(s: &mut MemStore, head: &SourceHead, zones: Option<&SourceZones>, data: &[u8]) {
    let bytes = file_bytes(FileKind::Source, head, zones, data);
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    s.make_dir(vol, Dir::Tapes).unwrap();
    let f = chimera_core::storage::source_file(head.sref);
    s.write(vol, f, &mut |w| w.put(&bytes)).unwrap();
}

const CHUNK: usize = 64;

/// Feeds `bytes` in `CHUNK`s: the chunk the first refusal came from, and
/// it, or `finish`'s answer.
fn feed(stem: u32, bytes: &[u8]) -> Result<SourceHead, (usize, PutRefusal)> {
    let mut c = SourceCheck::new(stem, bytes.len() as u32).map_err(|e| (0, e))?;
    for (i, b) in bytes.chunks(CHUNK).enumerate() {
        c.feed(b).map_err(|e| (i, e))?;
    }
    c.finish().map_err(|e| (usize::MAX, e))
}

/// The chunk holding byte `at`.
fn chunk_of(at: usize) -> usize {
    at / CHUNK
}

/// `SourceCheck` refuses at the record that shows it, before `finish`: a
/// kind that isn't `Source` at the header, a head naming another stem at
/// the head, samples past the head's frames at the record that overruns,
/// short of them at the end, and samples that don't hash to the stem at
/// the last one.
#[test]
fn source_check_refuses_early() {
    let (h, z, data) = zone_set(6, &zones_of(3, 700));
    let good = file_bytes(FileKind::Source, &h, Some(&z), &data);
    assert_eq!(feed(h.sref.crc, &good), Ok(h));
    let head_end = 28 + 4 + 34;
    let last_data = good.len() - 4 - 1;

    let project = file_bytes(FileKind::Project, &h, Some(&z), &data);
    assert_eq!(
        feed(h.sref.crc, &project),
        Err((0, PutRefusal::Kind)),
        "kind"
    );

    assert_eq!(
        feed(h.sref.crc ^ 1, &good),
        Err((chunk_of(head_end - 1), PutRefusal::Crc)),
        "a head under another stem"
    );

    let mut flipped = data.clone();
    flipped[100] ^= 1;
    let other = file_bytes(FileKind::Source, &h, Some(&z), &flipped);
    assert_eq!(
        feed(h.sref.crc, &other),
        Err((chunk_of(last_data), PutRefusal::Crc)),
        "samples off their CRC"
    );

    // Plain sources, so frames alone move: their CRC doesn't cover them.
    let (p, pdata) = whole(Shape::Plain, 1, 1_000);
    let plain = file_bytes(FileKind::Source, &p, None, &pdata);
    assert_eq!(feed(p.sref.crc, &plain), Ok(p));
    // One frame more: a pair more than the samples hold.
    let over = SourceHead {
        sref: SourceRef {
            frames: 1_001,
            ..p.sref
        },
        ..p
    };
    let bytes = file_bytes(FileKind::Source, &over, None, &pdata);
    let n = bytes.len().div_ceil(CHUNK) - 1;
    assert_eq!(
        feed(p.sref.crc, &bytes),
        Err((n, PutRefusal::Frames)),
        "frames + 1"
    );
    // Two fewer: the last record overruns them.
    let under = SourceHead {
        sref: SourceRef {
            frames: 998,
            ..p.sref
        },
        ..p
    };
    let bytes = file_bytes(FileKind::Source, &under, None, &pdata);
    let last = bytes.len() - 4 - 1;
    assert_eq!(
        feed(p.sref.crc, &bytes),
        Err((chunk_of(last), PutRefusal::Frames)),
        "frames − 2"
    );
    // A refusal sticks.
    let mut c = SourceCheck::new(p.sref.crc, project.len() as u32).unwrap();
    assert_eq!(c.feed(&project), Err(PutRefusal::Kind));
    assert_eq!(c.feed(&[]), Err(PutRefusal::Kind));
    assert_eq!(c.finish(), Err(PutRefusal::Kind));
}

/// A `Plain` or `Transwave` file's length gives its `held` back, at every
/// length a writer frames; a length no file has gives `None`.
#[test]
fn held_for_file_len_inverts_framing() {
    for frames in 1..=100_000 {
        let r = SourceRef {
            crc: 0,
            frames,
            rate: 24_000,
        };
        let len = source_file_min_len(r).unwrap();
        assert_eq!(
            held_for_file_len(len),
            Some(held_len(r).unwrap() as u32),
            "{frames}"
        );
    }
    for len in 0..=40_000u32 {
        if let Some(held) = held_for_file_len(len) {
            let frames = (held - DESC_LEN as u32) / 3 * 2;
            let r = SourceRef {
                crc: 0,
                frames,
                rate: 24_000,
            };
            assert_eq!(source_file_min_len(r), Some(len), "{len}");
        }
    }
    // What `write_source` writes is that length, both shapes.
    for (shape, count, frames) in [(Shape::Plain, 1, 777), (Shape::Transwave, 4, 4 * 1_216)] {
        let (h, data) = whole(shape, count, frames);
        let mut s = MemStore::new(1);
        let w = Card::new().run(&mut s, |s, r| write_source(s, r, &h, &data));
        assert_eq!(w.unwrap().result, Ok(Wrote::New));
        let vol = s.mount().unwrap();
        let mut size = 0;
        s.list(vol, Dir::Tapes, &mut |_, n| size = n).unwrap();
        assert_eq!(
            held_for_file_len(size),
            Some(held_len(h.sref).unwrap() as u32)
        );
    }
}

/// A 12-zone instrument with 1 024-frame heads (spec § 8) is one source:
/// one `SourceId`, one bit, one meter entry of 18 576 B, and six fit
/// beside NEW.
#[test]
fn zone_set_is_one_source() {
    let held_of = |seed: u8| {
        let (h, z, data) = zone_set(seed, &zones_of(12, 1_024));
        let l = SourceLayout::zoned(h, z).unwrap();
        let mut held = vec![0; l.held_len()];
        let at = write_descriptor(&l, &mut held);
        held[at..].copy_from_slice(&data);
        (h.sref, held)
    };
    let (r, held) = held_of(10);
    assert_eq!(held.len(), 18_556);
    assert_eq!(Cost::of(16 + held.len()).bytes(), 18_576);

    let (mut p, _) = Project::boxed();
    let id = p.source_store(r, &held).expect("fits beside NEW");
    let live = SourceId::ALL
        .into_iter()
        .filter(|&s| p.pool().source(s).is_some())
        .collect::<Vec<_>>();
    assert_eq!(live, [id], "one SourceId");
    assert_eq!(id.bit().count_ones(), 1);
    assert_eq!(p.pool().source(id).unwrap().cost().bytes(), 18_576);
    for seed in 11..16 {
        let (r, held) = held_of(seed);
        p.source_store(r, &held).expect("six beside NEW");
    }
}

/// A Sound's `ZoneTune` survives the card, and its table is checked.
#[test]
fn zone_tune_round_trips() {
    let tunes: Vec<(u16, u32)> = (0..12)
        .map(|i| (3_600 + 100 * i, (90 + i as u32) << 16))
        .collect();
    let mut s = Sound::init(EngineType::Algo);
    s.zone_tune = ZoneTune::new(&tunes);
    assert_eq!(s.zone_tune.unwrap().zones().collect::<Vec<_>>(), tunes);
    let back = decode(&encode(&s)).expect("decodes");
    assert!(back.bits_eq(&s));
    assert_eq!(back.zone_tune, s.zone_tune);
    let mut other = s.clone();
    other.zone_tune = ZoneTune::new(&tunes[..11]);
    assert!(!other.bits_eq(&s), "bits_eq sees the tuning");

    assert_eq!(ZoneTune::new(&[]), None);
    assert_eq!(ZoneTune::new(&[(0, 0); 25]), None);
    assert!(ZoneTune::new(&[(0, 0); 24]).is_some());
    assert_eq!(ZoneTune::from_record(&[0; 6 * 25]), None);
    let t = s.zone_tune.unwrap();
    assert_eq!(t.record().len(), 72, "6 B a zone");
    assert_eq!(ZoneTune::from_record(t.record()), Some(t));
    assert_eq!(ZoneTune::from_record(&t.record()[..71]), None);
    assert_eq!(ZoneTune::from_record(&[]), None);
}
