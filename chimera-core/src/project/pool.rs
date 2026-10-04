//! The project's Sounds: 128 slots of encoded Sounds in the arena (ADR
//! 0087), each with a generation that every store and clear moves, so a
//! Part can tell its slot was replaced. A Part decodes its Sound into its
//! own state; the pool keeps only the bytes.

use chimera_hal::store::{ByteSink, StoreError};

use crate::block::DiskCode;
use crate::name::SoundName;
use crate::params::EngineType;
use crate::preset::{MAX_SOUNDS, Sound};
use crate::storage::{
    Crc32, Event, FileError, ReadTag, RecordTag, RecordWriter, SoundCheck, encode_sound,
};

use super::arena::{ArenaWriter, Cost};
use super::ids::SlotId;

/// An entry's name, padded, before its records.
const NAME_LEN: usize = 16;

/// A record's tag and length, before its payload.
const HEAD_LEN: usize = 4;

/// Where a slot's bytes sit in the arena.
#[derive(Clone, Copy, Debug)]
struct SoundEntry {
    offset: u32,
    len: u32,
}

/// A store the arena had no room for: what it needed past the slot's old
/// Sound, and the gap there was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolFull {
    pub needs: Cost,
    pub free: Cost,
}

/// Only `project` stores or clears, so only it moves a generation:
///
/// ```compile_fail,E0624
/// use chimera_core::preset::Sound;
/// use chimera_core::project::{Pool, SlotId};
/// fn f(pool: &mut Pool, s: SlotId, sound: Sound) {
///     let _ = pool.store(s, &sound);
/// }
/// ```
pub struct Pool {
    slots: [Option<SoundEntry>; MAX_SOUNDS],
    gens: [u16; MAX_SOUNDS],
    arena: ArenaWriter,
}

impl Pool {
    /// Empty, every generation 0. The arena's Sounds are dropped; its
    /// sources stay.
    pub fn new(mut arena: ArenaWriter) -> Pool {
        arena.sounds_clear();
        Pool {
            slots: [None; MAX_SOUNDS],
            gens: [0; MAX_SOUNDS],
            arena,
        }
    }

    pub fn get(&self, s: SlotId) -> Option<EncodedSound<'_>> {
        let e = self.slots[s.index()]?;
        Some(EncodedSound {
            bytes: self.arena.bytes(e.offset, e.len as usize),
        })
    }

    pub fn generation(&self, s: SlotId) -> u16 {
        self.gens[s.index()]
    }

    pub fn first_free(&self) -> Option<SlotId> {
        SlotId::ALL
            .into_iter()
            .find(|s| self.slots[s.index()].is_none())
    }

    pub fn sound_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    /// The arena's bytes in use: Sounds and sources.
    pub fn used(&self) -> Cost {
        self.arena.used()
    }

    /// The gap: what an add may take.
    pub fn free(&self) -> Cost {
        self.arena.gap()
    }

    pub fn fits(&self, c: Cost) -> bool {
        c <= self.free()
    }

    /// Encodes `sound` into slot `s`, over what was there. Refused, with
    /// nothing moved, when it won't fit even with the old Sound's bytes
    /// given back.
    pub(in crate::project) fn store(&mut self, s: SlotId, sound: &Sound) -> Result<(), PoolFull> {
        let len = entry_len(sound);
        let old = self.slots[s.index()].map_or(0, |e| Cost::of(e.len as usize).bytes());
        let needs = Cost::of(len).bytes().saturating_sub(old);
        if needs > self.free().bytes() {
            return Err(PoolFull {
                needs: Cost::of(needs as usize),
                free: self.free(),
            });
        }
        self.drop_entry(s);
        let offset = self
            .arena
            .sound_alloc(len)
            .expect("checked against the gap");
        let bytes = self.arena.bytes_mut(offset, len);
        let (name, records) = bytes.split_at_mut(NAME_LEN);
        name.copy_from_slice(&sound.name.padded());
        let mut w = Fill(records);
        let wrote = encode_sound(sound, &mut RecordWriter::over(&mut w));
        assert!(wrote.is_ok() && w.0.is_empty(), "the count's length");
        let e = SoundEntry {
            offset,
            len: len as u32,
        };
        let check = EncodedSound {
            bytes: self.arena.bytes(offset, len),
        }
        .check();
        assert_eq!(check, Ok(()), "encode_sound wrote a Sound it refuses");
        self.slots[s.index()] = Some(e);
        self.bump(s);
        Ok(())
    }

    /// Moves the generation even when the slot is already empty.
    pub(in crate::project) fn clear(&mut self, s: SlotId) {
        self.drop_entry(s);
        self.bump(s);
    }

    /// Every slot cleared at once: a load or NEW about to refill.
    pub(in crate::project) fn clear_all(&mut self) {
        self.arena.sounds_clear();
        self.slots = [None; MAX_SOUNDS];
        for s in SlotId::ALL {
            self.bump(s);
        }
    }

    /// Frees the slot's bytes; the Sounds below move up over them.
    fn drop_entry(&mut self, s: SlotId) {
        let Some(e) = self.slots[s.index()].take() else {
            return;
        };
        let shift = self.arena.sound_free(e.offset, e.len as usize);
        for x in self.slots.iter_mut().flatten() {
            if x.offset < shift.from {
                x.offset += shift.by;
            }
        }
    }

    fn bump(&mut self, s: SlotId) {
        let g = &mut self.gens[s.index()];
        *g = g.wrapping_add(1);
    }
}

/// The bytes `sound` takes in the pool: its name, then its records.
pub(super) fn entry_len(sound: &Sound) -> usize {
    let mut n = Count(0);
    // `Count::put` is never Err.
    let _ = encode_sound(sound, &mut RecordWriter::over(&mut n));
    NAME_LEN + n.0
}

/// Counts bytes, keeping none.
struct Count(usize);

impl ByteSink for Count {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        self.0 += b.len();
        Ok(())
    }
}

/// Fills a slice front to back; `Full` past its end.
struct Fill<'a>(&'a mut [u8]);

impl ByteSink for Fill<'_> {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        if b.len() > self.0.len() {
            return Err(StoreError::Full);
        }
        let (head, rest) = core::mem::take(&mut self.0).split_at_mut(b.len());
        head.copy_from_slice(b);
        self.0 = rest;
        Ok(())
    }
}

/// Compares what it is given with `expect`, front to back; stops at the
/// first difference.
struct Same<'a> {
    expect: &'a [u8],
}

impl ByteSink for Same<'_> {
    fn put(&mut self, b: &[u8]) -> Result<(), StoreError> {
        match self.expect.strip_prefix(b) {
            Some(rest) => {
                self.expect = rest;
                Ok(())
            }
            None => Err(StoreError::Full),
        }
    }
}

/// A slot's Sound as the pool holds it: the padded name, then the records
/// `encode_sound` wrote. A Part decodes it; nothing else needs a `Sound`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodedSound<'a> {
    bytes: &'a [u8],
}

impl EncodedSound<'_> {
    pub fn name(&self) -> SoundName {
        <&[u8; NAME_LEN]>::try_from(&self.bytes[..NAME_LEN])
            .ok()
            .and_then(|n| SoundName::from_padded(n).ok())
            .unwrap_or_else(Sound::init_name)
    }

    /// From the first record, `Engine`, which `encode_sound` writes first.
    pub fn engine(&self) -> EngineType {
        self.bytes
            .get(NAME_LEN + HEAD_LEN)
            .and_then(|&c| EngineType::from_disk_code(c))
            .unwrap_or(EngineType::Algo)
    }

    pub fn cost(&self) -> Cost {
        Cost::of(self.bytes.len())
    }

    /// `sound_crc` of the Sound decoded: the same bytes, hashed.
    pub fn crc(&self) -> u32 {
        let mut c = Crc32::new();
        c.update(self.bytes);
        c.finish()
    }

    /// The bytes, name first.
    pub fn bytes(&self) -> &[u8] {
        self.bytes
    }

    /// The records alone, as `encode_sound` wrote them.
    pub(crate) fn records(&self) -> &[u8] {
        &self.bytes[NAME_LEN..]
    }

    /// `s` encodes to these bytes: a Part on this Sound is Clean.
    pub fn matches(&self, s: &Sound) -> bool {
        let (name, records) = self.bytes.split_at(NAME_LEN);
        if name != s.name.padded() {
            return false;
        }
        let mut same = Same { expect: records };
        encode_sound(s, &mut RecordWriter::over(&mut same)).is_ok() && same.expect.is_empty()
    }

    /// Checks, then decodes onto `s`: a refusal leaves `s` as it was.
    pub fn decode_into(&self, s: &mut Sound) -> Result<(), FileError> {
        self.check()?;
        let mut c = SoundCheck::begin(Some(self.name()));
        self.each(|e| c.step(e, Some(&mut *s)))?;
        c.finish(Some(s))
    }

    /// The Sound, decoded (host tests).
    #[cfg(any(test, feature = "test-support"))]
    pub fn to_sound(&self) -> Sound {
        let mut s = Sound::neutral(EngineType::Algo);
        self.decode_into(&mut s).expect("the pool's bytes decode");
        s
    }

    fn check(&self) -> Result<(), FileError> {
        let mut c = SoundCheck::begin(Some(self.name()));
        self.each(|e| c.step(e, None))?;
        c.finish(None)
    }

    /// Each record, in order.
    fn each(&self, mut f: impl FnMut(Event<'_>) -> Result<(), FileError>) -> Result<(), FileError> {
        let mut r = &self.bytes[NAME_LEN..];
        while let [t0, t1, l0, l1, rest @ ..] = r {
            let len = usize::from(u16::from_le_bytes([*l0, *l1]));
            let tag = RecordTag::from_code(u16::from_le_bytes([*t0, *t1]));
            let (Some(tag), Some(p)) = (tag, rest.get(..len)) else {
                return Err(FileError::Corrupt);
            };
            f(Event::Record(ReadTag::Known(tag), p))?;
            r = &rest[len..];
        }
        if r.is_empty() {
            Ok(())
        } else {
            Err(FileError::Corrupt)
        }
    }
}
