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
use crate::addr::{BlockRef, Blocks};
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

fn put_scene(w: &mut RecordWriter<'_>, at: u8, s: &Scene) -> Result<(), StoreError> {
    let rings =
        (s.rings.iter().zip(RingIdx::ALL)).map(|(r, i)| (BlockRef::OrbitRing(i), r as &dyn Block));
    for (b, blk) in core::iter::once((BlockRef::OrbitChords, &s.chords as &dyn Block)).chain(rings)
    {
        let mut r = RecordBuf::new();
        r.u8(at);
        encode_block(b, blk, &mut r);
        w.put(RecordTag::OrbitBlock, r.as_slice())?;
    }
    for (i, sr) in s.rings.iter().enumerate() {
        let ring = &sr.ring;
        let mut r = RecordBuf::new();
        r.u8(at);
        r.u8(i as u8);
        r.u32(ring.hits_mask().bits());
        r.u8(ring.pool.len() as u8);
        for n in ring.pool.notes().iter().chain(&ring.notes) {
            r.u8(n.get());
        }
        w.put(RecordTag::OrbitNotes, r.as_slice())?;
    }
    for seg in 0..MAX_SEGS {
        let (played, bass, tones) = match s.chords.played[seg] {
            Some(c) => (PLAYED, c.bass().disk_code(), tones_of(c)),
            None => (UNPLAYED, 0, 0),
        };
        let [lo, hi] = tones.to_le_bytes();
        let degree = s.chords.degrees[seg].disk_code();
        w.put(
            RecordTag::OrbitChord,
            &[at, seg as u8, degree, played, bass, lo, hi],
        )?;
    }
    w.put(RecordTag::OrbitScale, &encode_scale(at, s.chords.custom))
}

/// Bit n: pitch class n.
fn tones_of(c: Chord) -> u16 {
    c.tones().iter().fold(0, |m, pc| m | 1 << pc.disk_code())
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
    let mut r = RecordBuf::new();
    r.u8(current.get());
    for at in 0..STORED as u8 {
        r.u32(scene(at).seed);
    }
    w.put(RecordTag::Orbit, r.as_slice())?;
    w.put(RecordTag::Tempo, &f32::from(bpm.get()).to_le_bytes())
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

/// One ORBIT record into `song` (pass 2); pass 1 has nothing to check.
pub(crate) fn read(tag: RecordTag, p: &[u8], song: Option<&mut OrbitSong>) {
    let Some(song) = song else { return };
    match tag {
        RecordTag::Tempo => {
            if let Ok(&b) = <&[u8; 4]>::try_from(p) {
                song.bpm = decode_tempo(f32::from_le_bytes(b));
            }
        }
        RecordTag::Orbit => {
            let Ok(&[current, ref seeds @ ..]) = <&[u8; ORBIT_LEN]>::try_from(p) else {
                return;
            };
            song.set_current(SceneIdx::new(current).unwrap_or(SceneIdx::ALL[0]));
            for (at, &seed) in (0..).zip(seeds.as_chunks::<4>().0) {
                if let Some(s) = song.stored_mut(at) {
                    s.seed = u32::from_le_bytes(seed);
                }
            }
        }
        RecordTag::OrbitBlock => {
            if let Some((&at, body)) = p.split_first()
                && let Some(s) = song.stored_mut(at)
            {
                // A block of the wrong shape changes nothing: skipped.
                let _ = decode_block(body, MIGRATIONS, TRANSLATIONS, Some(s as &mut dyn Blocks));
            }
        }
        RecordTag::OrbitNotes => notes(p, song),
        RecordTag::OrbitChord => chord(p, song),
        RecordTag::OrbitScale => {
            if let Some((at, set)) = decode_scale(p)
                && let Some(s) = song.stored_mut(at)
            {
                s.chords.custom = set;
            }
        }
        _ => {}
    }
}

/// `OrbitNotes`: scene, ring, the hits `u32`, the pool's length and notes,
/// then 32 step notes. A mask is taken only as the fill's own hits moved
/// (`with_mask`), a pool only whole (`Pool::new`), a step note only if it
/// is one; the rest keep the base's.
fn notes(p: &[u8], song: &mut OrbitSong) {
    let &[at, ring, m0, m1, m2, m3, len, ref rest @ ..] = p else {
        return;
    };
    let len = usize::from(len);
    let (Some(s), Some(r)) = (song.stored_mut(at), RingIdx::new(ring)) else {
        return;
    };
    if rest.len() != len + STEPS {
        return;
    }
    let ring = &mut s.rings[r.get() as usize].ring;
    let (pool, steps) = rest.split_at(len);
    let bits = u32::from_le_bytes([m0, m1, m2, m3]);
    if let Some(m) = HitMask::new(ring.fill().steps(), bits)
        && let Some(laid) = ring.with_mask(m)
    {
        *ring = laid;
    }
    let mut read = [Note::MIN; MAX_POOL];
    if len <= MAX_POOL
        && pool
            .iter()
            .zip(&mut read)
            .all(|(&b, n)| Note::new(b).map(|x| *n = x).is_some())
        && let Some(taken) = Pool::new(&read[..len])
    {
        ring.pool = taken;
    }
    for (n, &b) in ring.notes.iter_mut().zip(steps) {
        if let Some(x) = Note::new(b) {
            *n = x;
        }
    }
}

/// `OrbitChord`: scene, segment, degree, played, bass, tones `u16`. A
/// degree this firmware lacks keeps the base's; a played chord that can't
/// be one reads as none.
fn chord(p: &[u8], song: &mut OrbitSong) {
    let &[at, seg, degree, played, bass, lo, hi] = p else {
        return;
    };
    let seg = usize::from(seg);
    let Some(s) = song.stored_mut(at).filter(|_| seg < MAX_SEGS) else {
        return;
    };
    if let Some(d) = Degree::from_disk_code(degree) {
        s.chords.degrees[seg] = d;
    }
    s.chords.played[seg] = match played {
        PLAYED => played_chord(bass, u16::from_le_bytes([lo, hi])),
        _ => None,
    };
}
