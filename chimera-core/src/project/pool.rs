//! The project's Sounds: 128 slots of encoded Sounds in the arena (ADR
//! 0087), each with a generation that every store and clear moves, so a
//! Part can tell its slot was replaced. A Part decodes its Sound into its
//! own state; the pool keeps only the bytes.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::store::{ByteSink, StoreError};

use crate::block::DiskCode;
use crate::name::SoundName;
use crate::params::EngineType;
use crate::preset::{MAX_SOUNDS, Sound};
use crate::storage::{
    Crc32, Event, FileError, ReadTag, RecordTag, RecordWriter, SoundCheck, encode_sound,
};

use super::arena::{ArenaWriter, Cost, Shift};
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
    index: Index,
    arena: ArenaWriter,
    /// A load's slot, decoded whole before it is encoded in: here, in
    /// AXI, not on the stack. Nothing reads it but the load.
    scratch: Sound,
}

/// Where each slot's bytes sit, and its generation.
struct Index {
    slots: [Option<SoundEntry>; MAX_SOUNDS],
    gens: [u16; MAX_SOUNDS],
}

impl Pool {
    /// Empty, every generation 0, built in place: no `Pool` passes
    /// through the stack. The arena's Sounds are dropped; its sources stay.
    pub fn init_in_place(slot: &mut MaybeUninit<Pool>, mut arena: ArenaWriter) -> &mut Pool {
        arena.sounds_clear();
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; every slot, the generations,
        // the arena and the scratch are written once before
        // `assume_init_mut`.
        unsafe {
            let slots = addr_of_mut!((*p).index.slots).cast::<Option<SoundEntry>>();
            for i in 0..MAX_SOUNDS {
                slots.add(i).write(None);
            }
            addr_of_mut!((*p).index.gens).write([0; MAX_SOUNDS]);
            addr_of_mut!((*p).arena).write(arena);
            // ponytail: built by value, left to the optimiser to write in
            // place (`neutral_params` returns its snapshot whole);
            // stack-check guards the frame.
            addr_of_mut!((*p).scratch).write(Sound::neutral(EngineType::Algo));
            slot.assume_init_mut()
        }
    }

    pub fn get(&self, s: SlotId) -> Option<EncodedSound<'_>> {
        let e = self.index.slots[s.index()]?;
        Some(EncodedSound {
            bytes: self.arena.bytes(e.offset, e.len as usize),
        })
    }

    pub fn generation(&self, s: SlotId) -> u16 {
        self.index.gens[s.index()]
    }

    pub fn first_free(&self) -> Option<SlotId> {
        SlotId::ALL
            .into_iter()
            .find(|s| self.index.slots[s.index()].is_none())
    }

    pub fn sound_count(&self) -> usize {
        self.index.slots.iter().filter(|s| s.is_some()).count()
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

    /// The meter's parts: Sounds, live sources, pending holes. They sum
    /// to `used`.
    pub fn split(&self) -> (Cost, Cost, Cost) {
        let (sounds, pending) = (self.arena.sounds(), self.arena.pending());
        let sources = self.used().bytes() - sounds.bytes() - pending.bytes();
        (sounds, Cost::of(sources as usize), pending)
    }

    pub fn source_count(&self) -> usize {
        self.arena.source_count()
    }

    /// What storing `sound` in `s` takes from the gap: its cost less the
    /// slot's old Sound's.
    pub fn needs(&self, s: SlotId, sound: &Sound) -> Cost {
        self.index.needs_len(s, entry_len(sound))
    }

    /// `needs` for a Sound already counted at `len` bytes.
    pub(super) fn needs_len(&self, s: SlotId, len: usize) -> Cost {
        self.index.needs_len(s, len)
    }

    /// Encodes `sound` into slot `s`, over what was there. Refused, with
    /// nothing moved, when it won't fit even with the old Sound's bytes
    /// given back.
    pub(in crate::project) fn store(&mut self, s: SlotId, sound: &Sound) -> Result<(), PoolFull> {
        self.index.store(&mut self.arena, s, sound)
    }

    /// Slot `s` takes an entry's bytes as they are: no encode.
    pub(in crate::project) fn store_bytes(
        &mut self,
        s: SlotId,
        bytes: &[u8],
    ) -> Result<(), PoolFull> {
        self.index
            .store_with(&mut self.arena, s, bytes.len(), |to| {
                to.copy_from_slice(bytes)
            })
    }

    /// The load's scratch Sound.
    pub(in crate::project) fn scratch(&mut self) -> &mut Sound {
        &mut self.scratch
    }

    /// `store` of the scratch Sound.
    pub(in crate::project) fn store_scratch(&mut self, s: SlotId) -> Result<(), PoolFull> {
        self.index.store(&mut self.arena, s, &self.scratch)
    }

    /// Moves the generation even when the slot is already empty.
    pub(in crate::project) fn clear(&mut self, s: SlotId) {
        self.index.drop_entry(&mut self.arena, s);
        self.index.bump(s);
    }

    /// Every slot cleared at once: a load or NEW about to refill.
    pub(in crate::project) fn clear_all(&mut self) {
        self.arena.sounds_clear();
        self.index.slots = [None; MAX_SOUNDS];
        for s in SlotId::ALL {
            self.index.bump(s);
        }
    }
}

impl Index {
    fn needs_len(&self, s: SlotId, len: usize) -> Cost {
        let old = self.slots[s.index()].map_or(0, |e| Cost::of(e.len as usize).bytes());
        Cost::of(Cost::of(len).bytes().saturating_sub(old) as usize)
    }

    fn store(&mut self, arena: &mut ArenaWriter, s: SlotId, sound: &Sound) -> Result<(), PoolFull> {
        self.store_with(arena, s, entry_len(sound), |to| write_entry(to, sound))
    }

    /// `len` bytes for slot `s`, filled by `fill`.
    fn store_with(
        &mut self,
        arena: &mut ArenaWriter,
        s: SlotId,
        len: usize,
        fill: impl FnOnce(&mut [u8]),
    ) -> Result<(), PoolFull> {
        let needs = self.needs_len(s, len);
        if needs > arena.gap() {
            return Err(PoolFull {
                needs,
                free: arena.gap(),
            });
        }
        // Into the gap first, so a refusal can't cost the old Sound; only
        // a Sound that needs the old one's bytes frees them first.
        let offset = match arena.sound_alloc(len) {
            Some(at) => {
                fill(arena.bytes_mut(at, len));
                // The new entry is the lowest: the free moves it up too.
                match self.drop_entry(arena, s) {
                    Some(shift) if at < shift.from => at + shift.by,
                    _ => at,
                }
            }
            None => {
                self.drop_entry(arena, s);
                let Some(at) = arena.sound_alloc(len) else {
                    unreachable!("needs <= gap: the old Sound's bytes made room")
                };
                fill(arena.bytes_mut(at, len));
                at
            }
        };
        debug_assert_eq!(
            EncodedSound {
                bytes: arena.bytes(offset, len),
            }
            .check(),
            Ok(()),
            "encode_sound wrote a Sound it refuses"
        );
        self.slots[s.index()] = Some(SoundEntry {
            offset,
            len: len as u32,
        });
        self.bump(s);
        Ok(())
    }

    /// Frees the slot's bytes; the Sounds below move up over them, as the
    /// shift says.
    fn drop_entry(&mut self, arena: &mut ArenaWriter, s: SlotId) -> Option<Shift> {
        let e = self.slots[s.index()].take()?;
        let shift = arena.sound_free(e.offset, e.len as usize);
        for x in self.slots.iter_mut().flatten() {
            if x.offset < shift.from {
                x.offset += shift.by;
            }
        }
        Some(shift)
    }

    fn bump(&mut self, s: SlotId) {
        let g = &mut self.gens[s.index()];
        *g = g.wrapping_add(1);
    }
}

/// `sound`'s name and records into `to`, which is its `entry_len`.
fn write_entry(to: &mut [u8], sound: &Sound) {
    let (name, records) = to.split_at_mut(NAME_LEN);
    name.copy_from_slice(&sound.name.padded());
    let mut w = Fill(records);
    let wrote = encode_sound(sound, &mut RecordWriter::over(&mut w));
    debug_assert!(wrote.is_ok() && w.0.is_empty(), "the count's length");
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

impl<'a> EncodedSound<'a> {
    /// `bytes` as an entry: padded name, then records.
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }
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
