//! ORBIT in a project file (spec § 8, ADR 0078), after the six Parts: per
//! scene, the slots holding one and then `live` (8), its blocks, each
//! ring's notes, each segment's chord and its CUSTOM set; then the playing
//! scene with the nine seeds, then the tempo.
//!
//! A reader takes what it can. A record of the wrong shape is skipped and
//! a bad value keeps the neutral base's, so ORBIT never costs a project
//! its Parts, and pass 1 has nothing to refuse.

use chimera_hal::store::StoreError;
use chimera_theory::{Chord, Degree, Note, PitchClass};

use super::clock::Bpm;
use super::euclid::HitMask;
use super::scene::{NEUTRAL, OrbitSong, STORED_LIVE, Scene, SceneIdx, decode_scale, encode_scale};
use super::walk::Pool;
use super::{MAX_POOL, MAX_SEGS, MAX_STEPS, RingIdx};
use crate::addr::{BlockRead, BlockRef, Blocks};
use crate::block::{Block, DiskCode};
use crate::storage::{
    MIGRATIONS, RecordBuf, RecordTag, RecordWriter, TRANSLATIONS, decode_block, encode_block,
};

const STEPS: usize = MAX_STEPS as usize;
/// Scenes a file holds: the slots, then `live`.
const STORED: usize = STORED_LIVE as usize + 1;
/// `Orbit`: the playing scene, then each stored scene's seed.
const ORBIT_LEN: usize = 1 + 4 * STORED;

/// A segment with no chord played: degree only.
const UNPLAYED: u8 = 0;
const PLAYED: u8 = 1;

/// `OrbitBlock`: scene `at`, then block `b`'s payload.
fn block_payload(at: u8, b: BlockRef, blk: &dyn Block) -> RecordBuf {
    let mut r = RecordBuf::new();
    r.u8(at);
    encode_block(b, blk, &mut r);
    r
}

/// `OrbitNotes`: scene, ring, the hits `u32`, the pool's length and notes,
/// then the 32 step notes.
fn notes_payload(at: u8, r: RingIdx, s: &Scene) -> RecordBuf {
    let ring = &s.rings[r.get() as usize].ring;
    let mut p = RecordBuf::new();
    p.u8(at);
    p.u8(r.get());
    p.u32(ring.hits_mask().bits());
    p.u8(ring.pool.len() as u8);
    for n in ring.pool.notes().iter().chain(&ring.notes) {
        p.u8(n.get());
    }
    p
}

/// `OrbitChord`: scene, segment, degree, played, bass, tones `u16`.
fn chord_payload(at: u8, seg: usize, s: &Scene) -> [u8; 7] {
    let (played, bass, tones) = match s.chords.played[seg] {
        Some(c) => (PLAYED, c.bass().disk_code(), tones_of(c)),
        None => (UNPLAYED, 0, 0),
    };
    let [lo, hi] = tones.to_le_bytes();
    let degree = s.chords.degrees[seg].disk_code();
    [at, seg as u8, degree, played, bass, lo, hi]
}

fn put_scene(w: &mut RecordWriter<'_>, at: u8, s: &Scene) -> Result<(), StoreError> {
    let r = block_payload(at, BlockRef::OrbitChords, &s.chords);
    w.put(RecordTag::OrbitBlock, r.as_slice())?;
    for (r, sr) in RingIdx::ALL.into_iter().zip(&s.rings) {
        let p = block_payload(at, BlockRef::OrbitRing(r), sr);
        w.put(RecordTag::OrbitBlock, p.as_slice())?;
    }
    for r in RingIdx::ALL {
        w.put(RecordTag::OrbitNotes, notes_payload(at, r, s).as_slice())?;
    }
    for seg in 0..MAX_SEGS {
        w.put(RecordTag::OrbitChord, &chord_payload(at, seg, s))?;
    }
    w.put(RecordTag::OrbitScale, &encode_scale(at, s.chords.custom))
}

/// Bit n: pitch class n.
fn tones_of(c: Chord) -> u16 {
    c.tones().iter().fold(0, |m, pc| m | 1 << pc.disk_code())
}

/// `Orbit`: the playing scene, then each stored scene's seed.
fn orbit_payload(current: SceneIdx, seed: &dyn Fn(u8) -> u32) -> RecordBuf {
    let mut r = RecordBuf::new();
    r.u8(current.get());
    for at in 0..STORED as u8 {
        r.u32(seed(at));
    }
    r
}

fn tempo_payload(bpm: Bpm) -> [u8; 4] {
    f32::from(bpm.get()).to_le_bytes()
}

/// The scenes `scene` gives, the slots in `saved` and then `live`, then
/// the playing scene, the seeds and the tempo. A slot left out is
/// neutral, as the reader's base is.
fn put_song<'a>(
    w: &mut RecordWriter<'_>,
    saved: u8,
    scene: &dyn Fn(u8) -> &'a Scene,
    current: SceneIdx,
    bpm: Bpm,
) -> Result<(), StoreError> {
    for at in 0..STORED as u8 {
        if at == STORED_LIVE || saved & 1 << at != 0 {
            put_scene(w, at, scene(at))?;
        }
    }
    let r = orbit_payload(current, &|at| scene(at).seed);
    w.put(RecordTag::Orbit, r.as_slice())?;
    w.put(RecordTag::Tempo, &tempo_payload(bpm))
}

/// `song`'s records. While a switch waits, as if it had landed: the queued
/// slot playing, and `live` its copy (Decisions, Review Focus 7).
pub(crate) fn encode_orbit(song: &OrbitSong, w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    let (current, live) = song.landing();
    let scene = |at: u8| match SceneIdx::new(at) {
        Some(n) => song.scene(n),
        None => live,
    };
    put_song(w, song.saved(), &scene, current, song.bpm)
}

/// `OrbitSong::NEW`'s records, a scene at a time: no song in RAM.
pub(crate) fn encode_new_orbit(w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    let first = SceneIdx::ALL[0];
    let init = Scene::init();
    let scene = |at: u8| {
        if at == first.get() || at == STORED_LIVE {
            &init
        } else {
            &NEUTRAL
        }
    };
    put_song(w, 1 << first.get(), &scene, first, Bpm::DEFAULT)
}

/// The tempo's `f32`, rounded and held to BPM's range; NaN is 120.
fn decode_tempo(v: f32) -> Bpm {
    if v.is_nan() {
        return Bpm::DEFAULT;
    }
    // `as` saturates: ±inf and the huge are the ends of the range.
    Bpm::clamped(libm::roundf(v) as u16)
}

/// A played chord from its bass and tones: its notes rising from the bass,
/// as `Chord::from_notes` built it. None without its bass or under two
/// tones.
fn played_chord(bass: u8, tones: u16) -> Option<Chord> {
    let bass = PitchClass::from_disk_code(bass)?;
    let b = bass.disk_code();
    if tones >> b & 1 == 0 {
        return None;
    }
    let low = Note::new(60 + b)?;
    let mut notes = [low; 12];
    let mut n = 1;
    for up in 1..12u8 {
        if tones >> ((b + up) % 12) & 1 == 1 {
            notes[n] = Note::new(low.get() + up)?;
            n += 1;
        }
    }
    Chord::from_notes(&notes[..n])
}

/// One ORBIT record into `song` (pass 2; pass 1 has nothing to check).
/// True when it was taken whole: the song now writes this very record
/// back. False when it was skipped, or a value in it was defaulted,
/// clamped or dropped (ADR 0078: such a load is never Saved).
pub(crate) fn read(tag: RecordTag, p: &[u8], song: Option<&mut OrbitSong>) -> bool {
    let Some(song) = song else { return true };
    match tag {
        RecordTag::Tempo => {
            let Ok(&b) = <&[u8; 4]>::try_from(p) else {
                return false;
            };
            song.bpm = decode_tempo(f32::from_le_bytes(b));
            tempo_payload(song.bpm) == b
        }
        RecordTag::Orbit => {
            let Ok(&[current, ref seeds @ ..]) = <&[u8; ORBIT_LEN]>::try_from(p) else {
                return false;
            };
            song.set_current(SceneIdx::new(current).unwrap_or(SceneIdx::ALL[0]));
            for (at, &seed) in (0..).zip(seeds.as_chunks::<4>().0) {
                if let Some(s) = song.stored_mut(at) {
                    s.seed = u32::from_le_bytes(seed);
                }
            }
            let seed = |at| song.stored(at).map_or(0, |s| s.seed);
            orbit_payload(song.current(), &seed).as_slice() == p
        }
        RecordTag::OrbitBlock => {
            let Some((&at, body)) = p.split_first() else {
                return false;
            };
            let Some(s) = song.stored_mut(at) else {
                return false;
            };
            // A block of the wrong shape changes nothing: skipped.
            if decode_block(body, MIGRATIONS, TRANSLATIONS, Some(s as &mut dyn Blocks)).is_err() {
                return false;
            }
            let Some(b) = body.first().and_then(|&c| BlockRef::from_disk_code(c)) else {
                return false;
            };
            s.block(b)
                .is_some_and(|blk| block_payload(at, b, blk).as_slice() == p)
        }
        RecordTag::OrbitNotes => notes(p, song),
        RecordTag::OrbitChord => chord(p, song),
        RecordTag::OrbitScale => {
            let Some((at, set)) = decode_scale(p) else {
                return false;
            };
            let Some(s) = song.stored_mut(at) else {
                return false;
            };
            s.chords.custom = set;
            encode_scale(at, set) == p
        }
        _ => true,
    }
}

/// `OrbitNotes`: a mask is taken only as the fill's own hits moved
/// (`with_mask`), a pool only whole (`Pool::new`), a step note only if it
/// is one; the rest keep the base's.
fn notes(p: &[u8], song: &mut OrbitSong) -> bool {
    let &[at, ring, m0, m1, m2, m3, len, ref rest @ ..] = p else {
        return false;
    };
    let len = usize::from(len);
    let (Some(s), Some(r)) = (song.stored_mut(at), RingIdx::new(ring)) else {
        return false;
    };
    if rest.len() != len + STEPS {
        return false;
    }
    let ring = &mut s.rings[r.get() as usize].ring;
    let (pool, steps) = rest.split_at(len);
    let bits = u32::from_le_bytes([m0, m1, m2, m3]);
    if let Some(m) = HitMask::new(ring.fill().steps(), bits)
        && let Some(laid) = ring.with_mask(m)
    {
        *ring = laid;
    }
    if let Some(taken) = pool_of(pool) {
        ring.pool = taken;
    }
    for (n, &b) in ring.notes.iter_mut().zip(steps) {
        if let Some(x) = Note::new(b) {
            *n = x;
        }
    }
    notes_payload(at, r, s).as_slice() == p
}

/// A pool read whole: 1..=8 notes, each one.
fn pool_of(bytes: &[u8]) -> Option<Pool> {
    let mut read = [Note::MIN; MAX_POOL];
    if bytes.len() > MAX_POOL {
        return None;
    }
    for (n, &b) in read.iter_mut().zip(bytes) {
        *n = Note::new(b)?;
    }
    Pool::new(&read[..bytes.len()])
}

/// `OrbitChord`: a degree this firmware lacks keeps the base's; a played
/// chord that can't be one reads as none.
fn chord(p: &[u8], song: &mut OrbitSong) -> bool {
    let &[at, seg, degree, played, bass, lo, hi] = p else {
        return false;
    };
    let seg = usize::from(seg);
    let Some(s) = song.stored_mut(at).filter(|_| seg < MAX_SEGS) else {
        return false;
    };
    if let Some(d) = Degree::from_disk_code(degree) {
        s.chords.degrees[seg] = d;
    }
    s.chords.played[seg] = match played {
        PLAYED => played_chord(bass, u16::from_le_bytes([lo, hi])),
        _ => None,
    };
    chord_payload(at, seg, s) == *p
}
