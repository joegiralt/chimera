//! ORBIT in the project file (spec § 8, Task 18): the eight slots and
//! `live`, drift and all, the seeds, the playing scene and the tempo, as
//! records after the six Parts. A bad record or value loads as the neutral
//! default; nothing in ORBIT's records can cost the Parts.

mod common;
mod screen;

use chimera_core::block::Block;
use chimera_core::orbit::chords::ScaleKind;
use chimera_core::orbit::clock::Bpm;
use chimera_core::orbit::runner::Captured;
use chimera_core::orbit::scene::{OrbitSong, Refused, Scene, SceneIdx, SceneRing};
use chimera_core::preset::Sound;
use chimera_core::project::{Confirmed, PartId, Project, SceneSlot};
use chimera_core::storage::{FileError, MAX_RECORD_LEN, RecordTag};
use chimera_theory::{Chord, Note, PitchClass, Scale};
use common::codec_util::{check_project_decode, fix_crc};
use common::orbit::OrbitRig;
use common::project::{decode, encode, same};

const S: [SceneIdx; 8] = SceneIdx::ALL;
const ORBIT_TAGS: [RecordTag; 6] = [
    RecordTag::Tempo,
    RecordTag::OrbitBlock,
    RecordTag::OrbitNotes,
    RecordTag::OrbitChord,
    RecordTag::Orbit,
    RecordTag::OrbitScale,
];
const PART: u16 = 0x8008;
/// `live`, as a record's scene byte.
const LIVE: u8 = 8;

/// A file's header and its records, `(tag, payload)`.
fn records(f: &[u8]) -> (Vec<u8>, Vec<(u16, Vec<u8>)>) {
    let end = f.len() - 4;
    let mut v = vec![];
    let mut p = 28;
    while p < end {
        let tag = u16::from_le_bytes([f[p], f[p + 1]]);
        let len = usize::from(u16::from_le_bytes([f[p + 2], f[p + 3]]));
        v.push((tag, f[p + 4..p + 4 + len].to_vec()));
        p += 4 + len;
    }
    (f[..28].to_vec(), v)
}

/// The header, the records and a fresh CRC.
fn rebuild(header: &[u8], recs: &[(u16, Vec<u8>)]) -> Vec<u8> {
    let mut f = header.to_vec();
    for (tag, p) in recs {
        f.extend(tag.to_le_bytes());
        f.extend((p.len() as u16).to_le_bytes());
        f.extend(p);
    }
    f.extend([0; 4]);
    fix_crc(&mut f);
    f
}

fn is_orbit(tag: u16) -> bool {
    ORBIT_TAGS.iter().any(|t| t.code() == tag)
}

/// The file's ORBIT records, in order.
fn orbit_records(f: &[u8]) -> Vec<(u16, Vec<u8>)> {
    records(f).1.into_iter().filter(|r| is_orbit(r.0)).collect()
}

/// `encode(p)` with `change` applied to its records.
fn edited(p: &Project, change: impl FnOnce(&mut Vec<(u16, Vec<u8>)>)) -> Vec<u8> {
    let (h, mut recs) = records(&encode(p));
    change(&mut recs);
    rebuild(&h, &recs)
}

/// The index of the first record of `tag` whose payload starts `head`.
fn find(recs: &[(u16, Vec<u8>)], tag: RecordTag, head: &[u8]) -> usize {
    recs.iter()
        .position(|(t, p)| *t == tag.code() && p.starts_with(head))
        .unwrap_or_else(|| panic!("no {tag:?} {head:?}"))
}

/// `bytes` decoded over a fresh NEW.
fn loaded(bytes: &[u8]) -> Box<Project> {
    let (mut q, _) = Project::boxed();
    decode(bytes, &mut q).expect("loads");
    q
}

fn new_project() -> Box<Project> {
    Project::boxed().0
}

/// Each ring's drift as a scene holds it.
fn scene_drift(s: &Scene) -> Vec<(u32, Vec<u8>, Vec<u8>)> {
    s.rings
        .iter()
        .map(|r| {
            (
                r.ring.hits_mask().bits(),
                r.ring.pool.notes().iter().map(|n| n.get()).collect(),
                r.ring.notes.iter().map(|n| n.get()).collect(),
            )
        })
        .collect()
}

fn notes(n: &[u8]) -> Vec<Note> {
    n.iter().map(|&n| Note::new(n).unwrap()).collect()
}

/// NEW played until every ring has drifted (MUTATE 100), then: slots 2–8
/// each a variant with its own key, steps and seed, slot 4 on a CUSTOM
/// set, `live` with a played cluster (C C# D over C) on segment 2, at 97
/// BPM, slot 6 playing.
fn rich() -> Box<Project> {
    let mut rig = OrbitRig::new();
    for r in &mut rig.song().live.rings {
        r.set(SceneRing::MUTATE, 100.0);
        r.set(SceneRing::MUTE, 0.0);
    }
    let start = scene_drift(&rig.project.orbit().live);
    rig.play(true);
    rig.run_loops(3);
    rig.play(false);
    rig.run(4);
    let live = rig.project.orbit().live;
    // Rings 1, 2 and 5 can drift; 3 and 4 are one hit of one note.
    let drifted = scene_drift(&live);
    for r in [0, 1, 4] {
        assert_ne!(start[r], drifted[r], "ring {} drifted", r + 1);
    }
    let mut p = rig.project;
    for n in 1..8 {
        let mut s = live;
        s.chords.tonic = PitchClass::ALL[n];
        s.seed = 0x5EED_0000 + n as u32;
        s.rings[n % 5].set(SceneRing::STEPS, n as f32);
        if n == 3 {
            s.chords.scale = ScaleKind::Custom;
            s.chords.custom = Scale::custom(&[
                chimera_theory::Interval::UNISON,
                chimera_theory::Interval::MINOR_THIRD,
                chimera_theory::Interval::from_semitones(6),
            ]);
        }
        let song = p.orbit_mut();
        let was = song.live;
        song.live = s;
        let c = Confirmed::<SceneSlot>::unasked(song, S[n]);
        song.overwrite_scene(c).expect("nothing queued");
        song.live = was;
    }
    let song = p.orbit_mut();
    song.live.chords.played[1] = Chord::from_notes(&notes(&[60, 61, 62]));
    // Rings 1 and 2 with their own uneven steps, so no step can stand in
    // for another.
    for (r, base) in [(0, 36u8), (1, 60)] {
        for (i, n) in song.live.rings[r].ring.notes.iter_mut().enumerate() {
            *n = Note::new(base + (i as u8 * 5 + r as u8) % 19).unwrap();
        }
    }
    song.live.seed = 0xCAFE_F00D;
    song.bpm = Bpm::new(97).unwrap();
    song.queue(S[5]);
    p
}

/// Every scene and `live`, with drift, seeds, a cluster chord, a CUSTOM
/// set and the tempo, bit for bit; a switch waiting is saved landed; each
/// record under `MAX_RECORD_LEN`.
#[test]
fn orbit_records_round_trip() {
    let p = rich();
    let bytes = encode(&p);
    for (tag, payload) in orbit_records(&bytes) {
        assert!(payload.len() < MAX_RECORD_LEN, "{tag:#x}");
    }
    let q = loaded(&bytes);
    let (a, b) = (p.orbit(), q.orbit());
    // Saved as landed: slot 6 playing, `live` its copy.
    assert_eq!(b.current(), S[5]);
    assert_eq!(b.queued(), None);
    assert_eq!(b.live, *a.scene(S[5]));
    for n in S {
        assert_eq!(b.scene(n), a.scene(n), "slot {}", n.get() + 1);
    }
    assert_eq!(b.bpm, a.bpm);
    assert_eq!(b.saved().bits(), a.saved().bits());
    assert_eq!(b.saved().bits(), 0xFF);
    assert_eq!(encode(&q), bytes);

    // Nothing queued: `live` itself, its cluster and seed.
    let current = a.current();
    let mut p = p;
    p.orbit_mut().queue(current);
    let live = p.orbit().live;
    let q = loaded(&encode(&p));
    same(&p, &q);
    assert_eq!(q.orbit().live, live);
    assert_eq!(
        q.orbit().live.chords.played[1],
        Chord::from_notes(&notes(&[60, 61, 62]))
    );
    assert_eq!(q.orbit().live.seed, 0xCAFE_F00D);
    assert_eq!(scene_drift(&q.orbit().live), scene_drift(&live));
}

/// The records follow Part 6, land in the song and leave Part 6's Sound
/// as it was. Before the six Parts, an ORBIT record is `Corrupt`.
#[test]
fn orbit_records_after_part_six_reach_orbit() {
    let p = rich();
    let (_, recs) = records(&encode(&p));
    let first = recs.iter().position(|r| is_orbit(r.0)).expect("ORBIT's");
    let last_part = recs.iter().rposition(|r| r.0 == PART).unwrap();
    assert_eq!(recs[last_part].1[0], 5);
    assert!(first > last_part);
    assert!(recs[first..].iter().all(|r| is_orbit(r.0)), "ORBIT's last");
    // Per scene: the blocks, the notes, the chords, the set.
    let kinds: Vec<u16> = recs[first..first + 20].iter().map(|r| r.0).collect();
    let mut want = vec![RecordTag::OrbitBlock.code(); 6];
    want.extend([RecordTag::OrbitNotes.code(); 5]);
    want.extend([RecordTag::OrbitChord.code(); 8]);
    want.push(RecordTag::OrbitScale.code());
    assert_eq!(kinds, want);
    let tail: Vec<u16> = recs[recs.len() - 2..].iter().map(|r| r.0).collect();
    assert_eq!(tail, [RecordTag::Orbit.code(), RecordTag::Tempo.code()]);

    let q = loaded(&encode(&p));
    let six = PartId::ALL[5];
    assert!(q.part(six).sound.bits_eq(&p.part(six).sound));
    assert_eq!(q.part(six).mix, p.part(six).mix);
    assert_eq!(q.orbit().bpm, p.orbit().bpm);

    for tag in ORBIT_TAGS {
        let payload = recs.iter().find(|r| r.0 == tag.code()).unwrap().1.clone();
        // Before FX, and between Parts 3 and 4.
        let parts: Vec<usize> = recs
            .iter()
            .enumerate()
            .filter(|r| r.1.0 == PART)
            .map(|r| r.0)
            .collect();
        for at in [0, parts[3]] {
            let f = edited(&p, |r| r.insert(at, (tag.code(), payload.clone())));
            assert_eq!(
                decode(&f, &mut new_project()),
                Err(FileError::Corrupt),
                "{tag:?} at {at}"
            );
        }
    }
}

/// A Sound file holding an ORBIT record is `Corrupt`.
#[test]
fn orbit_records_are_corrupt_in_a_sound() {
    let f = common::codec_util::encode(&Sound::init(chimera_core::params::EngineType::Algo));
    let (h, all) = records(&f);
    for tag in ORBIT_TAGS {
        let mut recs = all.clone();
        recs.insert(1, (tag.code(), vec![0; 4]));
        assert_eq!(
            common::codec_util::decode(&rebuild(&h, &recs)).unwrap_err(),
            FileError::Corrupt,
            "{tag:?}"
        );
    }
}

/// A load, then a save with the scenes untouched: the same ORBIT bytes.
#[test]
fn orbit_records_survive_a_resave() {
    let mut p = rich();
    let current = p.orbit().current();
    p.orbit_mut().queue(current);
    let bytes = encode(&p);
    let q = loaded(&bytes);
    let again = encode(&q);
    assert_eq!(orbit_records(&again), orbit_records(&bytes));
    assert_eq!(again, bytes);
}

/// The projects-core v1 fixture, saved before ORBIT: neutral scenes, 120
/// BPM, slot 1 playing, nothing saved.
#[test]
fn a_file_without_orbit_loads_neutral() {
    let f = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/v1/project.prj"
    ))
    .unwrap();
    assert!(orbit_records(&f).is_empty());
    let q = loaded(&f);
    let o = q.orbit();
    assert_eq!(o.live, Scene::neutral());
    for n in S {
        assert_eq!(*o.scene(n), Scene::neutral());
    }
    assert_eq!(o.bpm, Bpm::DEFAULT);
    assert_eq!((o.current(), o.queued(), o.saved().bits()), (S[0], None, 0));
}

/// A pool or hit mask that can't be one, a chord missing its bass or of
/// one tone, a degree past XII: each the default, never an error.
#[test]
fn bad_pool_mask_and_chord_take_defaults() {
    let p = new_project();
    let want = p.orbit().live;
    let without = |tag: RecordTag, head: &[u8]| {
        loaded(&edited(&p, |r| {
            let i = find(r, tag, head);
            r.remove(i);
        }))
        .orbit()
        .live
    };
    let with = |tag: RecordTag, head: &[u8], change: &dyn Fn(&mut Vec<u8>)| {
        loaded(&edited(&p, |r| {
            let i = find(r, tag, head);
            change(&mut r[i].1);
        }))
        .orbit()
        .live
    };
    // Ring 1's notes: scene, ring, mask u32, pool length, pool, 32 steps.
    let ring1 = [LIVE, 0];
    let neutral_notes = without(RecordTag::OrbitNotes, &ring1);
    assert_ne!(neutral_notes.rings[0].ring.pool, want.rings[0].ring.pool);
    for (what, change) in [
        (
            "pool of 0",
            &(|p: &mut Vec<u8>| p[6] = 0) as &dyn Fn(&mut Vec<u8>),
        ),
        ("pool of 9", &|p: &mut Vec<u8>| p[6] = 9),
        ("pool note 128", &|p: &mut Vec<u8>| p[7] = 128),
    ] {
        let got = with(RecordTag::OrbitNotes, &ring1, change);
        assert_eq!(
            got.rings[0].ring.pool, neutral_notes.rings[0].ring.pool,
            "{what}"
        );
    }
    // A step note past 127 keeps the default at that step only.
    let got = with(RecordTag::OrbitNotes, &ring1, &|p: &mut Vec<u8>| {
        let at = p.len() - 32;
        p[at] = 200;
    });
    assert_eq!(
        got.rings[0].ring.notes[0],
        neutral_notes.rings[0].ring.notes[0]
    );
    assert_eq!(got.rings[0].ring.notes[1..], want.rings[0].ring.notes[1..]);
    assert_eq!(got.rings[0].ring.pool, want.rings[0].ring.pool);
    // Ring 2 (8 steps, 3 hits): a mask with a hit past STEPS, or with
    // another count, is the fill's own.
    let ring2 = [LIVE, 1];
    let laid = want.rings[1].ring.fill().mask();
    for (what, bits) in [("past STEPS", 0x0000_0107u32), ("4 hits", 0x0000_000F)] {
        let got = with(RecordTag::OrbitNotes, &ring2, &|p: &mut Vec<u8>| {
            p[2..6].copy_from_slice(&bits.to_le_bytes())
        });
        assert_eq!(got.rings[1].ring.hits_mask(), laid, "{what}");
        assert_eq!(got.rings[1].ring.pool, want.rings[1].ring.pool, "{what}");
    }
    // A mask moved, hits kept: taken.
    let got = with(RecordTag::OrbitNotes, &ring2, &|p: &mut Vec<u8>| {
        p[2..6].copy_from_slice(&0x0000_0007u32.to_le_bytes())
    });
    assert_eq!(got.rings[1].ring.hits_mask().bits(), 0x7);

    // Segment 2's chord: scene, seg, degree, played, bass, tones u16.
    let seg2 = [LIVE, 1];
    let cluster = |p: &mut Vec<u8>, bass: u8, tones: u16| {
        p[3] = 1;
        p[4] = bass;
        p[5..7].copy_from_slice(&tones.to_le_bytes());
    };
    let got = with(RecordTag::OrbitChord, &seg2, &|p: &mut Vec<u8>| {
        cluster(p, 0, 0b111)
    });
    assert_eq!(
        got.chords.played[1],
        Chord::from_notes(&notes(&[60, 61, 62])),
        "C C# D over C"
    );
    for (what, bass, tones) in [
        ("no bass", 0, 0b110),
        ("one tone", 0, 0b1),
        ("bass past B", 12, 0b111),
    ] {
        let got = with(RecordTag::OrbitChord, &seg2, &|p: &mut Vec<u8>| {
            cluster(p, bass, tones)
        });
        assert_eq!(got.chords.played[1], None, "{what}");
        assert_eq!(got.chords.degrees, want.chords.degrees, "{what}");
    }
    let got = with(RecordTag::OrbitChord, &seg2, &|p: &mut Vec<u8>| p[2] = 12);
    assert_eq!(got.chords.degrees[1], Scene::neutral().chords.degrees[1]);
    // Any other length: the record is skipped.
    for tag in [RecordTag::OrbitNotes, RecordTag::OrbitChord] {
        let got = with(tag, &[LIVE, 0], &|p: &mut Vec<u8>| p.push(0));
        let skipped = without(tag, &[LIVE, 0]);
        assert_eq!(got, skipped, "{tag:?}");
    }
    // A scene past `live`: skipped.
    let got = loaded(&edited(&p, |r| {
        let i = find(r, RecordTag::OrbitBlock, &[LIVE]);
        r[i].1[0] = 9;
    }));
    let (c, n) = (got.orbit().live.chords, Scene::neutral().chords);
    assert_eq!(
        (c.tonic, c.scale, c.bars, c.segs),
        (n.tonic, n.scale, n.bars, n.segs)
    );
}

/// The tempo's `f32` rounds and clamps into BPM's range; NaN is 120.
#[test]
fn tempo_decodes_by_rounding() {
    let p = new_project();
    for (v, want) in [
        (119.6f32, 120),
        (97.4, 97),
        (1000.0, 300),
        (12.0, 30),
        (f32::NAN, 120),
        (f32::INFINITY, 300),
        (f32::NEG_INFINITY, 30),
    ] {
        let f = edited(&p, |r| {
            let i = find(r, RecordTag::Tempo, &[]);
            r[i].1 = v.to_le_bytes().to_vec();
        });
        assert_eq!(loaded(&f).orbit().bpm.get(), want, "{v}");
    }
    let f = edited(&p, |r| {
        let i = find(r, RecordTag::Tempo, &[]);
        r[i].1 = vec![0; 3];
    });
    assert_eq!(loaded(&f).orbit().bpm, Bpm::DEFAULT);
}

/// SCALE MINOR → CUSTOM seeds the set as MINOR; edited to DORIAN's
/// notes it stays DORIAN through a save and load, whichever record
/// comes first (Task 9 review M6).
#[test]
fn a_custom_set_equal_to_a_named_scale_survives() {
    let dorian = ScaleKind::Dorian.scale(Scale::MAJOR);
    let mut p = new_project();
    let c = &mut p.orbit_mut().live.chords;
    c.scale = ScaleKind::Minor;
    c.set(chimera_core::orbit::chords::ChordRing::SCALE, 10.0);
    assert_eq!((c.scale, c.custom), (ScaleKind::Custom, Scale::MINOR));
    c.custom = dorian;
    let q = loaded(&encode(&p));
    assert_eq!(q.orbit().live.chords.scale, ScaleKind::Custom);
    assert_eq!(q.orbit().live.chords.custom, dorian);
    // The set first, then the block.
    let f = edited(&p, |r| {
        let s = find(r, RecordTag::OrbitScale, &[LIVE]);
        let b = find(r, RecordTag::OrbitBlock, &[LIVE]);
        let set = r.remove(s);
        r.insert(b, set);
    });
    assert_eq!(loaded(&f).orbit().live.chords.custom, dorian);
}

/// A load writes every slot: a confirmation from before it refuses, and
/// the saved-slot mask is the file's.
#[test]
fn a_load_moves_every_slot_and_the_saved_mask() {
    let src = rich();
    let mut q = new_project();
    let asked: Vec<Confirmed<SceneSlot>> = S
        .iter()
        .map(|&n| Confirmed::<SceneSlot>::unasked(q.orbit(), n))
        .collect();
    decode(&encode(&src), &mut q).unwrap();
    assert_eq!(q.orbit().saved().bits(), 0xFF);
    for c in asked {
        assert_eq!(q.orbit_mut().overwrite_scene(c), Err(Refused::SlotChanged));
    }
    let asked = Confirmed::<SceneSlot>::unasked(q.orbit(), S[2]);
    decode(&encode(&new_project()), &mut q).unwrap();
    assert_eq!(q.orbit().saved().bits(), 1, "NEW: slot 1");
    assert_eq!(q.orbit_mut().clear_scene(asked), Err(Refused::SlotChanged));
}

/// Fuzz-ish: each kind of ORBIT record cut at every byte, and each bit of
/// it flipped. Never a panic, never an error: the Parts always load. The
/// song loaded is a fixed point (saved and loaded again it is itself), its
/// saved-slot mask is exactly the slots not neutral, and a cut record is
/// always a lossy load, the whole one never.
#[test]
fn damaged_orbit_records_never_cost_the_parts() {
    let p = rich();
    let (h, recs) = records(&encode(&p));
    let mut target = new_project();
    let mut kinds = 0;
    for tag in ORBIT_TAGS {
        // The last of its kind: `live`'s, or the song's.
        let i = recs.iter().rposition(|r| r.0 == tag.code()).unwrap();
        kinds += 1;
        let whole = recs[i].1.clone();
        let mut versions: Vec<Vec<u8>> = (0..=whole.len()).map(|n| whole[..n].to_vec()).collect();
        for byte in 0..whole.len() {
            for bit in 0..8 {
                let mut v = whole.clone();
                v[byte] ^= 1 << bit;
                versions.push(v);
            }
        }
        for v in versions {
            let cut = v.len() < whole.len();
            let is_whole = v == whole;
            let mut r = recs.clone();
            r[i].1 = v;
            let f = rebuild(&h, &r);
            assert!(check_project_decode(&f, &mut target), "{tag:?}");
            for id in PartId::ALL {
                assert!(target.part(id).sound.bits_eq(&p.part(id).sound));
            }
            let o = target.orbit();
            let mask = (0..8)
                .filter(|&n| !o.scene(S[n]).is_neutral())
                .fold(0, |m, n| m | 1 << n);
            assert_eq!(o.saved().bits(), mask, "{tag:?}");
            let (again, lossy) = loaded_lossy(&encode(&target));
            assert!(!lossy, "{tag:?}: written whole");
            assert_eq!(again.orbit(), target.orbit(), "{tag:?}: a fixed point");
            let lossy = common::codec_util::decode_project_lossy(&f, &mut target).unwrap();
            if cut {
                assert!(lossy, "{tag:?} cut");
            }
            if is_whole {
                assert!(!lossy, "{tag:?} whole");
            }
        }
    }
    assert_eq!(kinds, ORBIT_TAGS.len());
}

/// NEW saved and loaded sounds as NEW: the same events and the same
/// samples, sample for sample.
#[test]
fn new_sounds_the_same_after_a_save_and_load() {
    let mut a = OrbitRig::new();
    let mut b = OrbitRig::new();
    let bytes = encode(&b.project);
    let mut q = new_project();
    // Over anything: a rich project first.
    decode(&encode(&rich()), &mut q).unwrap();
    decode(&bytes, &mut q).unwrap();
    same(&a.project, &q);
    b.project = q;
    let mut events = (Vec::new(), Vec::new());
    for rig in [&mut a, &mut b] {
        rig.play(true);
    }
    for _ in 0..common::orbit::secs(9.0) {
        a.block();
        b.block();
        events.0.extend(a.sent.iter().copied());
        events.1.extend(b.sent.iter().copied());
    }
    assert!(!events.0.is_empty());
    assert_eq!(events.0, events.1);
    assert!(a.sum.iter().any(|x| x.abs() > 1e-3), "it sounds");
    assert!(
        a.sum
            .iter()
            .zip(&b.sum)
            .all(|(x, y)| x.to_bits() == y.to_bits()),
        "sample for sample"
    );
}

/// `OrbitSong::NEW` round-trips too: the in-place NEW and the constant
/// agree, and so does a file of it.
#[test]
fn new_song_round_trips() {
    let q = loaded(&encode(&new_project()));
    assert_eq!(*q.orbit(), OrbitSong::NEW);
    assert_eq!(q.orbit().saved().bits(), OrbitSong::NEW.saved().bits());
}

/// A captured chord, taken by the song, saves as exact tones.
#[test]
fn a_captured_chord_saves_as_its_tones() {
    let mut p = new_project();
    let mut c = Captured {
        count: 1,
        target: chimera_core::orbit::runner::Capture::Segment(2),
        notes: [Note::new(0).unwrap(); 8],
        len: 4,
    };
    for (i, n) in [50u8, 64, 67, 73].into_iter().enumerate() {
        c.notes[i] = Note::new(n).unwrap();
    }
    p.orbit_mut().live.take(&c);
    let played = p.orbit().live.chords.played[2].expect("taken");
    let q = loaded(&encode(&p));
    assert_eq!(q.orbit().live.chords.played[2], Some(played));
    assert_eq!(
        q.orbit().live.chords.played[2].unwrap().bass(),
        PitchClass::D
    );
}

#[cfg(feature = "orbit")]
mod unit {
    //! Through `UiState`, a card and the runner.

    use super::screen::{Input, Rig, feed, hold, tap};
    use super::*;
    use chimera_core::hw::CPU_HZ_REV_V;
    use chimera_core::hw::SampleBudget;
    use chimera_core::instrument::{AudioShared, Instrument};
    use chimera_core::orbit::runner::{OrbitRunner, OrbitView};
    use chimera_core::project::{ProjectFile, ProjectStatus};
    use chimera_core::ui::UiState;
    use chimera_core::ui::animation::UiTick;
    use chimera_core::ui::hold::HOLD_MS;
    use chimera_hal::ButtonId::{B2, B6, Menu, Plus, Seq};
    use chimera_hal::EncoderId::A;
    use chimera_hal::testkit::MemStore;

    /// The audio side: the runner in the published snapshot's epoch.
    struct Audio {
        shared: Box<AudioShared>,
        inst: Box<Instrument>,
        runner: Box<OrbitRunner>,
    }

    impl Audio {
        fn new() -> Audio {
            let sr = core::num::NonZeroU32::new(chimera_hal::SAMPLE_RATE).unwrap();
            Audio {
                shared: Box::default(),
                inst: Box::new(Instrument::new(
                    chimera_hal::SAMPLE_RATE,
                    SampleBudget::for_cpu(CPU_HZ_REV_V),
                )),
                runner: Box::new(OrbitRunner::new(sr)),
            }
        }

        fn view(&self) -> OrbitView {
            self.runner.view()
        }

        /// One frame: the view in, `input` (and the card), the publish,
        /// then `blocks` blocks.
        fn frame(&mut self, rig: &mut Rig<'_>, input: Input, blocks: usize) {
            rig.ui.set_orbit_view(&self.runner.view());
            rig.feed(input);
            self.shared
                .update_from(rig.ui.project().perf(), rig.epoch());
            rig.ui.publish_orbit(&mut self.shared.orbit);
            for _ in 0..blocks {
                let _ = self.runner.block(&mut self.inst, &self.shared);
            }
        }

        fn idle(&mut self, rig: &mut Rig<'_>, blocks: usize) {
            self.frame(rig, Input::default(), blocks);
        }

        /// Frames of 16 blocks until the view has seen `n` more loops,
        /// then one with none, so the drift is absorbed.
        fn loops(&mut self, rig: &mut Rig<'_>, n: u32) {
            assert!(self.view().running);
            let to = self.view().loops + n;
            for _ in 0..100 * n {
                if self.view().loops >= to {
                    break;
                }
                self.idle(rig, 16);
            }
            assert!(self.view().loops >= to, "{n} loops");
            self.idle(rig, 0);
        }

        /// MENU held: the quick save, no block between.
        fn quick_save(&mut self, rig: &mut Rig<'_>) {
            self.frame(rig, Input::press(Menu).at(0), 0);
            self.frame(rig, Input::held(Menu).at(HOLD_MS), 0);
            self.frame(rig, Input::release(Menu).at(HOLD_MS + 33), 0);
        }
    }

    /// A card holding NEW as ALPHA, loaded, at one bar a loop and 300 BPM
    /// with every ring sounding and MUTATE 100, inside ORBIT.
    fn unit(ui: &mut UiState) -> (Rig<'_>, Audio, ProjectFile) {
        let mut s = MemStore::new(1);
        let f = super::screen::put(&mut s, "ALPHA");
        let mut rig = Rig::new(ui, s);
        rig.load(f);
        let song = rig.ui.project_mut().orbit_mut();
        song.bpm = Bpm::clamped(Bpm::MAX);
        song.live.chords.bars = chimera_core::orbit::chords::Bars::ONE;
        for r in &mut song.live.rings {
            r.set(SceneRing::MUTATE, 100.0);
            r.set(SceneRing::MUTE, 0.0);
        }
        rig.hold(Seq);
        assert!(rig.ui.location().orbit().is_some());
        // The runner takes the load's epoch first.
        let mut audio = Audio::new();
        audio.idle(&mut rig, 2);
        audio.idle(&mut rig, 0);
        (rig, audio, f)
    }

    /// `f` loaded through LOAD (LOAD ANYWAY if it asks), from inside
    /// ORBIT, then back into ORBIT.
    fn reload(rig: &mut Rig<'_>, audio: &mut Audio, f: ProjectFile) {
        rig.tap(Menu);
        rig.to(&["PROJECT", "LOAD PROJECT"]);
        let l = rig.ui.listing();
        let i = (0..l.len())
            .find(|&i| l.entry(i).unwrap().file() == f)
            .expect("listed");
        rig.bar_to(i);
        rig.tap(chimera_hal::ButtonId::Edit);
        if rig.ui.prompt_open() {
            rig.feed(Input::turn(A, 1));
            rig.tap(Seq);
        }
        assert_eq!(rig.ui.project().meta().file(), Some(f));
        while rig.ui.in_settings() {
            rig.tap(Menu);
        }
        rig.hold(Seq);
        audio.idle(rig, 2);
        audio.idle(rig, 0);
    }

    fn view_drift(v: &OrbitView) -> Vec<(u32, Vec<u8>, Vec<u8>)> {
        v.rings
            .iter()
            .map(|r| {
                (
                    r.hits.bits(),
                    r.pool.notes().iter().map(|n| n.get()).collect(),
                    r.notes.iter().map(|n| n.get()).collect(),
                )
            })
            .collect()
    }

    /// 20 loops of drift, MENU hold, LOAD: `live` comes back as the
    /// runner was playing it, and ORBIT stopped.
    #[test]
    fn project_save_keeps_the_drift() {
        let mut ui = Box::new(UiState::new());
        let (mut rig, mut audio, f) = unit(&mut ui);
        audio.frame(&mut rig, Input::press(B6), 1);
        audio.frame(&mut rig, Input::release(B6), 1);
        assert!(audio.view().running);
        audio.loops(&mut rig, 20);
        let playing = view_drift(&audio.view());
        assert_eq!(scene_drift(&rig.ui.project().orbit().live), playing);
        audio.quick_save(&mut rig);
        assert_eq!(rig.ui.project_status(), ProjectStatus::Saved);

        // Played on, then loaded back.
        audio.loops(&mut rig, 2);
        assert_ne!(view_drift(&audio.view()), playing);
        rig.ui.update(UiTick::for_test());
        assert_eq!(
            rig.ui.project_status(),
            ProjectStatus::Modified,
            "drift absorbed"
        );
        reload(&mut rig, &mut audio, f);
        assert_eq!(scene_drift(&rig.ui.project().orbit().live), playing);
        assert!(!audio.view().running, "a load stops ORBIT");
        assert_eq!(view_drift(&audio.view()), playing, "the runner took it");
        audio.frame(&mut rig, Input::press(B6), 1);
        audio.frame(&mut rig, Input::release(B6), 1);
        assert!(audio.view().running, "one B6 starts it again");
    }

    /// Review Focus 7: MENU hold while a switch to scene 3 waits saves it
    /// landed: scene 3 playing, `live` its copy.
    #[test]
    fn quick_save_while_queued_saves_the_landing() {
        let mut ui = Box::new(UiState::new());
        let (mut rig, mut audio, f) = unit(&mut ui);
        let mut three = rig.ui.project().orbit().live;
        three.chords.tonic = PitchClass::E;
        let song = rig.ui.project_mut().orbit_mut();
        let was = song.live;
        song.live = three;
        let c = Confirmed::<SceneSlot>::unasked(song, S[2]);
        song.overwrite_scene(c).unwrap();
        song.live = was;
        audio.frame(&mut rig, Input::press(B6), 1);
        audio.frame(&mut rig, Input::release(B6), 1);
        audio.loops(&mut rig, 1);
        // SCN, SCENE → 3.
        for _ in 0..3 {
            audio.frame(&mut rig, Input::press(Plus), 0);
        }
        audio.frame(&mut rig, Input::turn(A, 2), 1);
        assert_eq!(rig.ui.project().orbit().queued(), Some(S[2]));
        assert_eq!(audio.view().cue, Some(S[2]), "waits for the loop");
        audio.quick_save(&mut rig);
        assert_eq!(rig.ui.project().orbit().queued(), Some(S[2]));
        assert_eq!(rig.ui.project().orbit().current(), S[0]);

        reload(&mut rig, &mut audio, f);
        let o = rig.ui.project().orbit();
        assert_eq!((o.current(), o.queued()), (S[2], None));
        assert_eq!(o.live, three);
        assert_eq!(audio.view().playing, S[2]);
    }

    /// ADR 0077 after a load: with a switch waiting before it, and the
    /// file playing another scene than the runner was, the knobs reach
    /// the runner again at once.
    #[test]
    fn knobs_reach_the_runner_after_a_load() {
        let mut ui = Box::new(UiState::new());
        let (mut rig, mut audio, f) = unit(&mut ui);
        // The file: scene 3 playing, in A minor.
        {
            let song = rig.ui.project_mut().orbit_mut();
            let c = Confirmed::<SceneSlot>::unasked(song, S[2]);
            song.overwrite_scene(c).unwrap();
            song.queue(S[2]);
        }
        audio.quick_save(&mut rig);
        rig.ui.project_mut().orbit_mut().queue(S[0]);
        // Playing scene 1 with a switch to scene 2 waiting.
        audio.frame(&mut rig, Input::press(B6), 1);
        audio.frame(&mut rig, Input::release(B6), 1);
        audio.loops(&mut rig, 1);
        for _ in 0..3 {
            audio.frame(&mut rig, Input::press(Plus), 0);
        }
        audio.frame(&mut rig, Input::turn(A, 1), 1);
        assert_eq!(audio.view().cue, Some(S[1]));
        assert_eq!(audio.view().playing, S[0]);

        reload(&mut rig, &mut audio, f);
        assert_eq!(audio.view().playing, S[2]);
        assert_eq!(rig.ui.project().orbit().queued(), None);
        assert_eq!(audio.view().chord.root(), PitchClass::A);
        rig.ui.project_mut().orbit_mut().live.chords.tonic = PitchClass::E;
        audio.idle(&mut rig, 1);
        assert_eq!(audio.view().chord.root(), PitchClass::E, "KEY reached it");
    }

    /// R9: STEPS turned on RING marks the project; a transport toggle doesn't. A
    /// fresh NEW, ORBIT records and all, is `Pristine`.
    #[test]
    fn orbit_edits_mark_the_project() {
        use chimera_core::orbit::channel::{Seq as LogSeq, receive};
        let mut ui = Box::new(UiState::new());
        assert_eq!(ui.project_status(), ProjectStatus::Pristine);
        hold(&mut ui, Seq);
        tap(&mut ui, B6);
        ui.update(UiTick::for_test());
        let sent = receive(ui.orbit().log(), &mut LogSeq::default());
        assert!(sent.flip, "one TogglePlay");
        assert_eq!(ui.project_status(), ProjectStatus::Pristine, "B6");
        tap(&mut ui, B6);
        ui.update(UiTick::for_test());
        assert_eq!(ui.project_status(), ProjectStatus::Pristine, "B6 again");

        // RING 2's RHYTHM: encoder A is STEPS, 8 → 7.
        tap(&mut ui, B2);
        feed(&mut ui, Input::turn(A, -1));
        let steps = ui.project().orbit().live.rings[1].ring.fill().steps();
        assert_eq!(steps.get(), 7, "the turn reached STEPS");
        ui.update(UiTick::for_test());
        assert_eq!(ui.project_status(), ProjectStatus::Modified, "STEPS");
    }
}

const ORBIT_FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/v1/orbit.prj");

/// `rich()` with nothing queued: `live` keeps its cluster.
fn fixture_project() -> Box<Project> {
    let mut p = rich();
    let current = p.orbit().current();
    p.orbit_mut().queue(current);
    p
}

/// Run once with `FIXTURE_WRITE=1 cargo test -p chimera-core --test
/// orbit_codec_test -- --ignored`, then commit the files: the fixture is
/// frozen from there on, never rewritten. It also writes NEW, the fuzz
/// seed beside it (`just fuzz` copies both into the corpus).
#[test]
#[ignore]
fn write_orbit_fixture() {
    assert!(
        std::env::var_os("FIXTURE_WRITE").is_some(),
        "FIXTURE_WRITE=1"
    );
    for (path, bytes) in [
        (ORBIT_FIXTURE, encode(&fixture_project())),
        (NEW_SEED, encode(&new_project())),
    ] {
        let path = std::path::Path::new(path);
        if !path.exists() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
    }
}

const NEW_SEED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fuzz/seeds/decode_project/new.prj"
);

/// The frozen fixture (ADR 0045 § Fixtures). The raw payloads below are
/// literals copied from the file: they pin the frozen bytes, so a later
/// reader must keep loading them; they exercise no encoder. Then the song
/// the file loads, field by field, and the same ORBIT records written back.
#[test]
fn the_orbit_fixture_loads_as_frozen() {
    let f = std::fs::read(ORBIT_FIXTURE).unwrap();
    assert_eq!(f.len(), 26_379);
    let recs = orbit_records(&f);
    let payload = |tag: RecordTag, head: &[u8]| recs[find(&recs, tag, head)].1.clone();
    // OrbitBlock: scene, block code 27, then (id, f32 or code u32)*: KEY A
    // (code 9), SCALE MINOR (1), BARS 4 (3.0), SEGS 4 (3.0).
    assert_eq!(
        payload(RecordTag::OrbitBlock, &[LIVE, 27]),
        [
            8, 27, 0, 9, 0, 0, 0, 1, 1, 0, 0, 0, 2, 0, 0, 64, 64, 3, 0, 0, 64, 64
        ]
    );
    // OrbitNotes: scene, ring, mask u32, pool length, pool, 32 steps.
    #[rustfmt::skip]
    let ring2 = [
        8, 1, 82, 0, 0, 0, 3, 62, 69, 72,
        61, 66, 71, 76, 62, 67, 72, 77, 63, 68, 73, 78, 64, 69, 74, 60,
        65, 70, 75, 61, 66, 71, 76, 62, 67, 72, 77, 63, 68, 73, 78, 64,
    ];
    assert_eq!(payload(RecordTag::OrbitNotes, &[LIVE, 1]), ring2);
    #[rustfmt::skip]
    let ring1 = [
        8, 0, 3, 0, 0, 0, 8, 24, 26, 33, 33, 41, 43, 45, 45,
        36, 41, 46, 51, 37, 42, 47, 52, 38, 43, 48, 53, 39, 44, 49, 54,
        40, 45, 50, 36, 41, 46, 51, 37, 42, 47, 52, 38, 43, 48, 53, 39,
    ];
    assert_eq!(payload(RecordTag::OrbitNotes, &[LIVE, 0]), ring1);
    // OrbitChord: scene, segment, degree VI (5), played, bass C, tones
    // C C# D.
    assert_eq!(
        payload(RecordTag::OrbitChord, &[LIVE, 1]),
        [8, 1, 5, 1, 0, 7, 0]
    );
    // OrbitScale: slot 4's CUSTOM set, the root, a minor third, a tritone.
    assert_eq!(payload(RecordTag::OrbitScale, &[3]), [3, 0b100_1001, 0]);
    // Orbit: slot 1 playing, then the seeds, slots 1–8 and `live`.
    let mut orbit = vec![0];
    orbit.extend(0x0B17_0001u32.to_le_bytes());
    for n in 1..8u32 {
        orbit.extend((0x5EED_0000 + n).to_le_bytes());
    }
    orbit.extend(0xCAFE_F00Du32.to_le_bytes());
    assert_eq!(payload(RecordTag::Orbit, &[]), orbit);
    // Tempo: 97.0 as f32 LE.
    assert_eq!(payload(RecordTag::Tempo, &[]), [0, 0, 194, 66]);

    let (q, lossy) = loaded_lossy(&f);
    assert!(!lossy, "a whole file");
    let o = q.orbit();
    assert_eq!(
        (o.current(), o.queued(), o.saved().bits(), o.bpm.get()),
        (S[0], None, 0xFF, 97)
    );
    let live = &o.live;
    assert_eq!(
        (live.chords.tonic, live.chords.scale),
        (PitchClass::A, ScaleKind::Minor)
    );
    assert_eq!(
        live.chords.played[1],
        Chord::from_notes(&notes(&[60, 61, 62]))
    );
    let seeds: Vec<u32> = S
        .iter()
        .map(|&n| o.scene(n).seed)
        .chain([live.seed])
        .collect();
    assert_eq!(
        seeds,
        [
            0x0B17_0001,
            0x5EED_0001,
            0x5EED_0002,
            0x5EED_0003,
            0x5EED_0004,
            0x5EED_0005,
            0x5EED_0006,
            0x5EED_0007,
            0xCAFE_F00D,
        ]
    );
    let steps = |r: usize| live.rings[r].ring.notes.map(|n| n.get());
    assert_eq!(steps(0)[..4], [36, 41, 46, 51]);
    assert_eq!(steps(1)[29..], [73, 78, 64]);
    let r2 = &live.rings[1].ring;
    assert_eq!(r2.hits_mask().bits(), 0b101_0010);
    assert_eq!(r2.pool.notes(), notes(&[62, 69, 72]));
    assert_eq!(o.scene(S[3]).chords.scale, ScaleKind::Custom);
    assert_eq!(o.scene(S[6]).chords.tonic, PitchClass::ALL[6]);
    // Written back: the same ORBIT bytes.
    assert_eq!(orbit_records(&encode(&q)), recs);
}

/// `bytes` decoded over a fresh NEW, and whether the load was lossy.
fn loaded_lossy(bytes: &[u8]) -> (Box<Project>, bool) {
    let (mut q, _) = Project::boxed();
    let lossy = common::codec_util::decode_project_lossy(bytes, &mut q).expect("loads");
    (q, lossy)
}

/// `bytes` as P0000001 on a fresh card, then loaded through
/// `load_project` and through boot: the status each shows.
fn status_after_load(bytes: &[u8]) -> [chimera_core::project::ProjectStatus; 2] {
    use chimera_core::project::{
        LoadLink, ProjectFile, boot_project, list_projects, load_project, project_file,
        project_status,
    };
    use chimera_core::storage::{Card, ProjectId, Side};
    use chimera_hal::store::{ByteSink, Dir, Store};
    use chimera_hal::testkit::MemStore;
    let mut s = MemStore::new(1);
    let vol = s.mount().unwrap();
    s.make_dir(vol, Dir::Chimera).unwrap();
    s.make_dir(vol, Dir::Projects).unwrap();
    let id = ProjectId::new(1).unwrap();
    let side = project_file(id).side(Side::A);
    s.write(vol, side, &mut |w: &mut dyn ByteSink| w.put(bytes))
        .unwrap();
    let mut file = None;
    let _ = list_projects(&mut Card::new(), &mut s, &mut |e| file = Some(e.file()));
    let file: ProjectFile = file.expect("listed");
    let (mut p, t) = Project::boxed();
    let go = common::project::confirm_load(&p, t, file);
    let out = load_project(&mut Card::new(), &mut s, &mut p, go, &LoadLink::new());
    assert!(out.note.is_none(), "{:?}", out.note);
    let loaded = project_status(&p, t);
    let (mut b, t) = Project::boxed();
    assert_eq!(
        boot_project(&mut Card::new(), &mut s, Some(id), &mut b),
        None
    );
    [loaded, project_status(&b, t)]
}

/// ADR 0078's ruling: a load that took less than the file, or that a save
/// would write otherwise, is never Saved. A whole file is; a damaged ORBIT
/// record or a file from before ORBIT reads Modified, and loads.
#[test]
fn a_lossy_load_reads_modified() {
    use chimera_core::project::ProjectStatus::{Modified, Saved};
    let p = fixture_project();
    assert_eq!(status_after_load(&encode(&p)), [Saved; 2]);
    let damaged = edited(&p, |r| {
        let i = find(r, RecordTag::OrbitNotes, &[LIVE, 1]);
        r[i].1[7] = 200;
    });
    assert_eq!(status_after_load(&damaged), [Modified; 2]);
    // Bits past B in a chord's tones, or past the 12 in a set: dropped.
    for (tag, head, at, bit) in [
        (RecordTag::OrbitChord, &[LIVE, 1][..], 6, 0x80),
        (RecordTag::OrbitScale, &[LIVE][..], 2, 0x10),
    ] {
        let f = edited(&p, |r| {
            let i = find(r, tag, head);
            r[i].1[at] |= bit;
        });
        assert!(loaded_lossy(&f).1, "{tag:?}");
        assert_eq!(status_after_load(&f), [Modified; 2], "{tag:?}");
    }
    let v1 = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/v1/project.prj"
    ))
    .unwrap();
    assert_eq!(status_after_load(&v1), [Modified; 2], "before ORBIT");
}
