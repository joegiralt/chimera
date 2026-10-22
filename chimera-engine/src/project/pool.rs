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

use super::arena::{ArenaWriter, Cost, GateAcked, MAX_SOURCES, Shift};
use super::ids::SlotId;
use super::sources::{
    Absence, SOURCE_HEAD, SourceEntry, SourceId, SourceRef, SourceSet, source_cost,
};
use crate::source_table::{Answer, GATE_ACK_TIMEOUT_MS, SourceTable};
use crate::stream::layout::StreamFile;
use crate::stream::registry::{RowRefused, StreamRegistry};

/// How long a load waits, at least, for the voices its kill fades to let
/// go of the old sources, then for their holes to close and their slots to
/// free: two or three blocks on the chip, one 10–20 ms desktop callback.
/// A longer audio period stretches it (`drain_ms`).
pub const LOAD_DRAIN_TIMEOUT_MS: u32 = 50;

/// A load's wait: `LOAD_DRAIN_TIMEOUT_MS`, or three of the audio's
/// callback periods if longer, so a long buffer never times a load out
/// (ADR 0099).
fn drain_ms(table: &SourceTable) -> u32 {
    LOAD_DRAIN_TIMEOUT_MS.max(table.period_ms().saturating_mul(3))
}

/// While `ms` haven't passed on `clock`; false at once without one.
fn deadline(clock: Option<fn() -> u32>, ms: u32) -> impl FnMut() -> bool {
    let t0 = clock.map(|c| c());
    move || match (clock, t0) {
        (Some(c), Some(t0)) => c().wrapping_sub(t0) < ms,
        _ => false,
    }
}

/// An entry's name, padded, before its records.
const NAME_LEN: usize = 16;

/// A record's tag and length, before its payload.
const HEAD_LEN: usize = 4;

/// Why a streamed source wasn't published.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamRefused {
    Pool(PoolFull),
    /// Every stream row taken (`STREAMS FULL`), or no registry.
    Full,
    /// A row the id never let go: the registry and the pool disagree.
    Taken,
}

impl From<PoolFull> for StreamRefused {
    fn from(e: PoolFull) -> Self {
        StreamRefused::Pool(e)
    }
}

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
/// use chimera_engine::preset::Sound;
/// use chimera_engine::project::{Pool, SlotId};
/// fn f(pool: &mut Pool, s: SlotId, sound: Sound) {
///     let _ = pool.store(s, &sound);
/// }
/// ```
pub struct Pool {
    index: Index,
    sources: Sources,
    /// What a gate waits by, in ms: the UI loop's clock. None in tests.
    clock: Option<fn() -> u32>,
    /// Factory Sounds the last NEW skipped, until the UI says so.
    pub(in crate::project) new_skipped: u8,
    arena: ArenaWriter,
    /// A load's slot, decoded whole before it is encoded in: here, in
    /// AXI, not on the stack. Nothing reads it but the load.
    scratch: Sound,
}

/// A source slot.
#[derive(Clone, Copy, Debug)]
enum SrcSlot {
    Empty,
    /// Held: its bytes in the arena, its row published.
    Live(SourceEntry),
    /// Kept by ref with no bytes (its card file missing, damaged, too big,
    /// or unread), costing nothing; why, for its SOURCE cell (#372).
    Absent(SourceRef, Absence),
    /// Out of the index, its row still live until its hole compacts: a
    /// voice may hold it (ADR 0087's retirement).
    Retired {
        offset: u32,
    },
    /// Its bytes gone and its row empty: free once no voice holds it, two
    /// blocks after audio block `since` began.
    Draining {
        since: u32,
    },
}

/// The source slots and their generations.
struct Sources {
    slots: [SrcSlot; MAX_SOURCES],
    gens: [u16; MAX_SOURCES],
    /// Ids whose stream row a failed publish couldn't free (a stale
    /// ring's card read held it): freed by `drain_retired`, the id kept
    /// from `free_source` till then.
    owed: SourceSet,
}

/// Where each slot's bytes sit, and its generation.
struct Index {
    slots: [Option<SoundEntry>; MAX_SOUNDS],
    gens: [u16; MAX_SOUNDS],
}

// `init_in_place` writes each field: a new one fails to build here first.
crate::in_place::field_list!(Pool => Pool { index, sources, clock, new_skipped, arena, scratch });
crate::in_place::field_list!(Index => Index { slots, gens });

impl Pool {
    /// Empty, every generation 0, built in place: no `Pool` passes
    /// through the stack. The arena's Sounds are dropped; its sources stay.
    pub fn init_in_place(slot: &mut MaybeUninit<Pool>, mut arena: ArenaWriter) -> &mut Pool {
        arena.sounds_clear();
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; every field (`field_list!`
        // above names them: the index's slots and generations, the
        // sources, the clock, the skip count, the arena and the scratch) is
        // written once before `assume_init_mut`.
        unsafe {
            let slots = addr_of_mut!((*p).index.slots).cast::<Option<SoundEntry>>();
            for i in 0..MAX_SOUNDS {
                slots.add(i).write(None);
            }
            addr_of_mut!((*p).index.gens).write([0; MAX_SOUNDS]);
            addr_of_mut!((*p).sources).write(Sources {
                slots: [SrcSlot::Empty; MAX_SOURCES],
                gens: [0; MAX_SOURCES],
                owed: SourceSet::EMPTY,
            });
            addr_of_mut!((*p).clock).write(None);
            addr_of_mut!((*p).new_skipped).write(0);
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

    /// The bytes the sources hold, pending holes included. Tests only: the
    /// meter reads `split`.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn source_used(&self) -> Cost {
        Cost::of((self.used().bytes() - self.arena.sounds().bytes()) as usize)
    }

    /// The arena's gap lent as scratch: the boot's card sweep keeps its
    /// reachability bitmap here (ADR 0094). Only while no source is
    /// published, so nothing the audio reads sits near it.
    pub fn sweep_scratch(&mut self) -> &mut [u8] {
        assert!(
            self.arena.source_count() == 0 && self.arena.pending().bytes() == 0,
            "a source is published"
        );
        self.arena.gap_mut()
    }

    /// The audio's view of the arena.
    pub fn table(&self) -> &'static SourceTable {
        self.arena.table()
    }

    /// Source slot `s`, if it holds a source (absent included).
    pub fn source(&self, s: SourceId) -> Option<SourceEntry> {
        match self.sources.slots[s.index()] {
            SrcSlot::Live(e) => Some(e),
            SrcSlot::Absent(r, _) => Some(SourceEntry {
                r,
                held: 0,
                offset: 0,
            }),
            SrcSlot::Empty | SrcSlot::Retired { .. } | SrcSlot::Draining { .. } => None,
        }
    }

    pub fn source_generation(&self, s: SourceId) -> u16 {
        self.sources.gens[s.index()]
    }

    /// The source `r` resolves to.
    pub fn find_source(&self, r: SourceRef) -> Option<SourceId> {
        SourceId::ALL
            .into_iter()
            .find(|&s| self.source(s).is_some_and(|e| e.r == r))
    }

    /// Source `r` as the audio reads it; `None` when the pool doesn't hold
    /// it (an absent one's empty bytes don't read).
    pub fn source_view(
        &self,
        r: SourceRef,
    ) -> Option<crate::dsp::tape_engine::read::SourceView<'_>> {
        let e = self.source(self.find_source(r)?)?;
        crate::dsp::tape_engine::read::SourceView::new(self.entry_bytes(e))
    }

    /// `e`'s held bytes, past its head.
    fn entry_bytes(&self, e: SourceEntry) -> &[u8] {
        self.arena
            .bytes(e.offset + SOURCE_HEAD as u32, e.held as usize)
    }

    /// Source `r` is held as heads, the rest streamed (slice 1 § 5.3).
    pub fn streamed(&self, r: SourceRef) -> bool {
        self.find_source(r)
            .and_then(|s| self.source(s))
            .map(|e| self.entry_bytes(e))
            .and_then(crate::dsp::tape_engine::read::Descriptor::from_bytes)
            .is_some_and(|d| d.is_streamed())
    }

    /// Source `r`'s held bytes; `None` when the pool doesn't hold them.
    pub fn held(&self, r: SourceRef) -> Option<&[u8]> {
        let e = self.source(self.find_source(r)?).filter(|e| !e.absent())?;
        Some(self.entry_bytes(e))
    }

    /// The registry streamed sources' files go in (slice 1 § 5.5).
    pub fn streams(&self) -> Option<&'static StreamRegistry> {
        self.arena.streams()
    }

    pub(in crate::project) fn set_streams(&mut self, r: &'static StreamRegistry) {
        self.arena.set_streams(r);
    }

    /// `source_commit` for a streamed source: its file's row goes in the
    /// registry first, so the card can read it before any voice finds the
    /// pool row (slice 1 § 5.5). Nothing is published on a refusal.
    pub(in crate::project) fn source_commit_streamed(
        &mut self,
        r: SourceRef,
        len: usize,
        file: StreamFile,
    ) -> Result<SourceId, StreamRefused> {
        let reg = self.arena.streams().ok_or(StreamRefused::Full)?;
        let first = |s| {
            reg.insert(s, file).map_err(|e| match e {
                RowRefused::Full => StreamRefused::Full,
                RowRefused::Taken => StreamRefused::Taken,
            })
        };
        let undo = |pool: &mut Pool, s: SourceId| {
            // SAFETY: `s`'s pool row was never published, so no voice
            // reached its stream row through it. A card read of a stale
            // ring that names `s` may hold the row: `clear` leaves a held
            // row and says so.
            if !unsafe { reg.clear(s) } {
                // Freed on a later pass, as `rehead_beside`'s drain.
                pool.sources.owed = pool.sources.owed.with(s);
            }
        };
        self.commit_with(r, len, first, undo)
    }

    /// Whether source `s`'s heads can be re-staged now (#598): no voice
    /// holds it (in place), or a free slot and stream row wait for them
    /// beside it.
    pub fn can_rehead(&self, s: SourceId) -> bool {
        self.table().sounding() & s.bit() == 0
            || self.free_source().is_some() && self.streams().is_some_and(|r| r.has_room())
    }

    /// Source `s`'s heads re-staged (slice 1 § 5.6): the `len` bytes
    /// `source_staging` holds become its heads. With no voice on `s` they
    /// replace its row in place, under the gate. While a note holds it
    /// they go in beside it (#598): a free id on the same stream file
    /// takes them, and `s` retires (ADR 0087), so its notes finish on the
    /// old heads, never cut, while new notes find the new id at once. The
    /// id the heads are under; `None`, nothing moved, when `s` sounds and
    /// no slot or stream row is free.
    pub(in crate::project) fn source_rehead(
        &mut self,
        s: SourceId,
        len: usize,
        within: &mut dyn FnMut() -> bool,
    ) -> Option<SourceId> {
        let SrcSlot::Live(old) = self.sources.slots[s.index()] else {
            return None;
        };
        if self.table().sounding() & s.bit() == 0 && self.rehead_in_place(s, old, len, within) {
            return Some(s);
        }
        self.rehead_beside(s, old, len)
    }

    /// `s`'s row rewritten under an acked gate; a nack (a note started on
    /// it) or a timeout moves nothing.
    fn rehead_in_place(
        &mut self,
        s: SourceId,
        old: SourceEntry,
        len: usize,
        within: &mut dyn FnMut() -> bool,
    ) -> bool {
        let table = self.table();
        let Ok(to) = self.arena.source_stage(SOURCE_HEAD + len) else {
            return false;
        };
        to[..SOURCE_HEAD].fill(0);
        to[..SourceRef::LEN].copy_from_slice(&old.r.to_bytes());
        let epoch = table.close_gate(s.bit());
        let mut done = false;
        if table.wait(epoch, within) == Answer::Acked
            && let Ok(offset) = self.arena.source_alloc(SOURCE_HEAD + len)
        {
            let held = len as u32;
            table.publish(s, offset + SOURCE_HEAD as u32, held, old.r.rate);
            self.arena
                .source_free_pending(old.offset, SOURCE_HEAD + old.held as usize);
            self.sources.slots[s.index()] = SrcSlot::Live(SourceEntry {
                r: old.r,
                held,
                offset,
            });
            done = true;
        }
        table.open_gate();
        done
    }

    /// The heads published under a free id streaming `s`'s file, then `s`
    /// retired: no gate, as nothing the audio reads moves.
    fn rehead_beside(&mut self, s: SourceId, old: SourceEntry, len: usize) -> Option<SourceId> {
        let reg = self.arena.streams()?;
        let file = *reg.get(s)?;
        let to = self.free_source()?;
        reg.insert(to, file).ok()?;
        if self.source_put(to, old.r, len, |_| Some(len)) != Ok(true) {
            // SAFETY: `to`'s pool row was never published, so no voice and
            // no card read has its stream row.
            if !unsafe { reg.clear(to) } {
                // A stale ring's read holds it: drained on a later pass,
                // the slot kept from reuse till then.
                let since = self.table().blocks();
                self.sources.slots[to.index()] = SrcSlot::Draining { since };
            }
            return None;
        }
        self.source_free(s);
        Some(to)
    }

    /// `source_rehead` against the pool's clock.
    pub(in crate::project) fn source_rehead_timed(
        &mut self,
        s: SourceId,
        len: usize,
    ) -> Option<SourceId> {
        self.source_rehead(s, len, &mut deadline(self.clock, GATE_ACK_TIMEOUT_MS))
    }

    /// Source `r` with its `held` bytes; an identical source already held
    /// is shared, and an absent one takes the bytes in its own slot.
    /// Refused, with nothing moved, when the gap can't take it, or when
    /// every slot is taken (`free` then reads 0).
    pub(in crate::project) fn source_store(
        &mut self,
        r: SourceRef,
        held: &[u8],
    ) -> Result<SourceId, PoolFull> {
        if held.is_empty() {
            return self.source_absent(r).ok_or(PoolFull {
                needs: Cost::default(),
                free: Cost::default(),
            });
        }
        let found = self.find_source(r);
        if let Some(s) = found.filter(|&s| self.source(s).is_some_and(|e| !e.absent())) {
            return Ok(s);
        }
        let Some(s) = found.or_else(|| self.free_source()) else {
            return Err(PoolFull {
                needs: source_cost(held.len()),
                free: Cost::default(),
            });
        };
        self.source_put(s, r, held.len(), |to| {
            to[..held.len()].copy_from_slice(held);
            Some(held.len())
        })?;
        Ok(s)
    }

    /// Absent source `s` takes the held bytes `fill` writes straight into
    /// the gap (a load's card read), at least `len`; `fill` says how many.
    /// They publish only when `fill` says they are good: `Ok(false)` leaves
    /// `s` absent. Refused, with nothing moved, when the gap can't take
    /// `len`.
    pub(in crate::project) fn source_fill(
        &mut self,
        s: SourceId,
        len: usize,
        fill: impl FnOnce(&mut [u8]) -> Option<usize>,
    ) -> Result<bool, PoolFull> {
        match self.source(s) {
            Some(e) if e.absent() => self.source_put(s, e.r, len, fill),
            _ => Ok(false),
        }
    }

    /// Slot `s` holds `r` with the bytes `fill` writes, at least `len`,
    /// staged in the whole gap (a zone set's layout is known only once its
    /// file is read) and published only if `fill` gives their length.
    fn source_put(
        &mut self,
        s: SourceId,
        r: SourceRef,
        len: usize,
        fill: impl FnOnce(&mut [u8]) -> Option<usize>,
    ) -> Result<bool, PoolFull> {
        let needs = source_cost(len);
        let free = self.arena.gap();
        if needs > free {
            return Err(PoolFull { needs, free });
        }
        // A free slot means a free arena row: retired holes keep theirs.
        let busy = |_| PoolFull {
            needs,
            free: Cost::default(),
        };
        let to = self
            .arena
            .source_stage(free.bytes() as usize)
            .map_err(busy)?;
        let (head, bytes) = to.split_at_mut(SOURCE_HEAD);
        head.fill(0);
        head[..SourceRef::LEN].copy_from_slice(&r.to_bytes());
        let least = len;
        let Some(len) = fill(bytes) else {
            return Ok(false);
        };
        debug_assert!(
            least <= len && len <= bytes.len(),
            "fill gave {len} B, at least {least}"
        );
        let offset = self.arena.source_alloc(SOURCE_HEAD + len).map_err(busy)?;
        let held = len as u32;
        self.sources.slots[s.index()] = SrcSlot::Live(SourceEntry { r, held, offset });
        self.sources.gens[s.index()] = self.sources.gens[s.index()].wrapping_add(1);
        self.table()
            .publish(s, offset + SOURCE_HEAD as u32, held, r.rate);
        Ok(true)
    }

    /// Source `r` as an absent entry: its ref kept, no bytes, cost 0. The
    /// slot `r` already has, if any; `None` with every slot taken.
    pub(in crate::project) fn source_absent(&mut self, r: SourceRef) -> Option<SourceId> {
        if let Some(s) = self.find_source(r) {
            return Some(s);
        }
        let s = self.free_source()?;
        self.sources.slots[s.index()] = SrcSlot::Absent(r, Absence::Missing);
        self.sources.gens[s.index()] = self.sources.gens[s.index()].wrapping_add(1);
        Some(s)
    }

    /// Why absent source `s` holds no bytes; `None` unless it is absent.
    pub fn absence(&self, s: SourceId) -> Option<Absence> {
        match self.sources.slots[s.index()] {
            SrcSlot::Absent(_, why) => Some(why),
            _ => None,
        }
    }

    pub(in crate::project) fn mark_absent(&mut self, s: SourceId, why: Absence) {
        if let SrcSlot::Absent(_, w) = &mut self.sources.slots[s.index()] {
            *w = why;
        }
    }

    /// Where a card-row load stages `len` held bytes, pass by pass (TAPE
    /// Task 17): past a row's head at the gap's bottom, as `source_put`
    /// lays them, with the arena offset there, so a load sees the gap
    /// move under it. `None` when they don't fit or every row is taken.
    pub(in crate::project) fn source_staging(&mut self, len: usize) -> Option<(u32, &mut [u8])> {
        let at = self.arena.src_end();
        let to = self.arena.source_stage(SOURCE_HEAD + len).ok()?;
        Some((at, &mut to[SOURCE_HEAD..]))
    }

    /// The `len` bytes `source_staging` holds, published as source `r`:
    /// into its absent slot, or a free one. An `r` already held is shared.
    pub(in crate::project) fn source_commit(
        &mut self,
        r: SourceRef,
        len: usize,
    ) -> Result<SourceId, PoolFull> {
        self.commit_with(r, len, |_| Ok(()), |_, _| {})
    }

    /// `source_commit`, with `first` run on the slot before its row is
    /// published: its refusal publishes nothing, and `undo` takes it back
    /// if the publish then fails.
    fn commit_with<E: From<PoolFull>>(
        &mut self,
        r: SourceRef,
        len: usize,
        first: impl FnOnce(SourceId) -> Result<(), E>,
        undo: impl FnOnce(&mut Self, SourceId),
    ) -> Result<SourceId, E> {
        let found = self.find_source(r);
        if let Some(s) = found.filter(|&s| self.source(s).is_some_and(|e| !e.absent())) {
            return Ok(s);
        }
        let s = found.or_else(|| self.free_source()).ok_or(PoolFull {
            needs: source_cost(len),
            free: Cost::default(),
        })?;
        first(s)?;
        if let Err(e) = self.source_put(s, r, len, |_| Some(len)) {
            undo(self, s);
            return Err(e.into());
        }
        Ok(s)
    }

    pub(in crate::project) fn free_source(&self) -> Option<SourceId> {
        SourceId::ALL.into_iter().find(|&s| {
            matches!(self.sources.slots[s.index()], SrcSlot::Empty)
                && !self.sources.owed.contains(s)
        })
    }

    /// Takes source `s` out of the index. Its bytes become a pending hole
    /// and its row stays live until `source_pass` compacts it: a voice
    /// may still hold it, and is never cut.
    pub(in crate::project) fn source_free(&mut self, s: SourceId) {
        let e = match self.sources.slots[s.index()] {
            SrcSlot::Live(e) => Some(e),
            SrcSlot::Absent(..) => None,
            _ => return,
        };
        self.sources.gens[s.index()] = self.sources.gens[s.index()].wrapping_add(1);
        self.sources.slots[s.index()] = if let Some(e) = e {
            self.arena
                .source_free_pending(e.offset, SOURCE_HEAD + e.held as usize);
            SrcSlot::Retired { offset: e.offset }
        } else {
            SrcSlot::Empty
        };
    }

    /// The ids at or above arena `offset`: what closing the hole there
    /// moves (ADR 0087's `above`).
    fn above(&self, offset: u32) -> u32 {
        SourceId::ALL
            .into_iter()
            .filter(|s| match self.sources.slots[s.index()] {
                SrcSlot::Live(e) => e.offset >= offset,
                SrcSlot::Retired { offset: o } => o >= offset,
                SrcSlot::Empty | SrcSlot::Absent(..) | SrcSlot::Draining { .. } => false,
            })
            .fold(0, |m, s| m | s.bit())
    }

    /// ADR 0087's sequence, whole, in one call, for the lowest pending
    /// hole with no source at or above it sounding (none: nothing): close
    /// the gate on those sources, wait for the audio's answer while
    /// `within` says so (it answers within two blocks), and on an ack close
    /// that hole and every one above it. A nack (a voice holds a moving id) or a timeout moves
    /// nothing. The gate is open again on return, always. True when a
    /// hole closed. Through `dyn`, so every caller shares one copy.
    #[inline(never)]
    pub(in crate::project) fn source_pass(&mut self, within: &mut dyn FnMut() -> bool) -> bool {
        self.drain_retired();
        let table = self.table();
        // The lowest hole nothing at or above sounds: a voice low in the
        // arena pins only the holes below it.
        let sounding = table.sounding();
        let Some((from, moving)) = self
            .arena
            .holes()
            .map(|h| (h.offset, self.above(h.offset)))
            .find(|&(_, above)| sounding & above == 0)
        else {
            return false;
        };
        let epoch = table.close_gate(moving);
        let mut moved = false;
        if table.wait(epoch, within) == Answer::Acked {
            // No voice holds a moving source, and none can start on one
            // until the gate opens: each hole from `from` up closes too.
            loop {
                let next = self.arena.holes().find(|h| h.offset >= from);
                let Some(h) = next else {
                    break;
                };
                let moves =
                    SourceId::ALL.map(|s| self.source_offset(s).filter(|_| moving & s.bit() != 0));
                // A re-head's old bytes (`source_rehead`, TAPE's only): no
                // slot's, and no row points at them, so nothing reads them.
                let orphan = cfg!(feature = "tape")
                    && SourceId::ALL
                        .iter()
                        .all(|&s| self.source_offset(s) != Some(h.offset));
                let ready = |o: u32| (orphan && o == h.offset) || moves.contains(&Some(o));
                let Some(shift) = self
                    .arena
                    .source_compact_at(h.offset, GateAcked::after_ack(ready))
                else {
                    break;
                };
                self.rebase_sources(shift, table.blocks());
                moved = true;
            }
        }
        table.open_gate();
        moved
    }

    /// `source_pass` against the pool's clock: `GATE_ACK_TIMEOUT_MS`, or
    /// one look with no clock.
    pub(in crate::project) fn source_pass_timed(&mut self) -> bool {
        self.source_pass(&mut deadline(self.clock, GATE_ACK_TIMEOUT_MS))
    }

    /// Whether the audio has `acked` a load's kill and no voice holds a
    /// source since, waiting `LOAD_DRAIN_TIMEOUT_MS` on the pool's clock
    /// (one look with no clock). The ack means a block began after the
    /// kill, so `sounding` isn't the block's before it. No reader: no audio
    /// to wait for.
    pub(in crate::project) fn quiet_timed(&self, acked: impl Fn() -> bool) -> bool {
        let table = self.table();
        let mut within = deadline(self.clock, drain_ms(table));
        while table.has_reader() && !(acked() && table.sounding() == 0) {
            if !within() {
                return false;
            }
            core::hint::spin_loop();
        }
        true
    }

    /// A load's retirement (ADR 0087): every source out of the index, then
    /// each hole closed and each slot freed, waiting `LOAD_DRAIN_TIMEOUT_MS`
    /// on the pool's clock. True once the arena holds no source and every
    /// slot is free, so pass 2 gets what pass 1 planned for.
    pub(in crate::project) fn sources_drain_timed(&mut self) -> bool {
        let mut within = deadline(self.clock, drain_ms(self.table()));
        for s in SourceId::ALL {
            self.source_free(s);
        }
        loop {
            self.source_pass(&mut within);
            self.drain_retired();
            if self.sources.owed == SourceSet::EMPTY
                && self
                    .sources
                    .slots
                    .iter()
                    .all(|s| matches!(s, SrcSlot::Empty))
            {
                return true;
            }
            if !within() {
                return false;
            }
            core::hint::spin_loop();
        }
    }

    /// Every source out of the index (NEW, a failed load): retired, and
    /// compacted at once unless one still sounds.
    pub(in crate::project) fn sources_clear(&mut self) {
        for s in SourceId::ALL {
            self.source_free(s);
        }
        self.source_pass_timed();
    }

    /// The clock `source_pass_timed` waits by, in ms.
    pub(in crate::project) fn set_clock(&mut self, now_ms: fn() -> u32) {
        self.clock = Some(now_ms);
    }

    /// A drained slot whose source no voice held since its row emptied
    /// is free again: a held note-on never finds its id reused. An owed
    /// stream row (`source_commit_streamed`'s undo) is freed too.
    fn drain_retired(&mut self) {
        let table = self.table();
        let (reader, blocks) = (table.has_reader(), table.blocks());
        let sounding = table.sounding();
        let streams = self.arena.streams();
        let Sources { slots, owed, .. } = &mut self.sources;
        for s in SourceId::ALL {
            let slot = &mut slots[s.index()];
            let drained = matches!(*slot, SrcSlot::Draining { since }
                if !reader || blocks.wrapping_sub(since) >= 2 && sounding & s.bit() == 0);
            if !drained && !owed.contains(s) {
                continue;
            }
            // SAFETY (ADR 0087's retire): a drained source was retired, its
            // hole closed under an acked gate that moved it, its table row
            // emptied two blocks ago and no voice held it since, so the
            // audio holds nothing of its row; an owed row's pool row was
            // never published. A card read in flight holds it (#597): left,
            // and freed on a later pass.
            let freed = match streams.filter(|_| cfg!(feature = "tape")) {
                Some(reg) => unsafe { reg.clear(s) },
                None => true,
            };
            if freed {
                if drained {
                    *slot = SrcSlot::Empty;
                }
                *owed = owed.without(s);
            }
        }
    }

    /// The index and the rows after `source_compact` closed the hole at
    /// `from`: its retired slot empties, every source above moves down.
    /// Under the closed gate.
    fn rebase_sources(&mut self, Shift { from, by }: Shift, since: u32) {
        let table = self.table();
        for s in SourceId::ALL {
            let slot = &mut self.sources.slots[s.index()];
            let base = match slot {
                SrcSlot::Retired { offset } if *offset == from => {
                    *slot = SrcSlot::Draining { since };
                    table.clear(s);
                    continue;
                }
                SrcSlot::Live(e) if e.offset > from => {
                    e.offset -= by;
                    e.offset
                }
                SrcSlot::Retired { offset } if *offset > from => {
                    *offset -= by;
                    *offset
                }
                _ => continue,
            };
            table.rebase(s, base + SOURCE_HEAD as u32);
        }
    }

    fn source_offset(&self, s: SourceId) -> Option<u32> {
        match self.sources.slots[s.index()] {
            SrcSlot::Live(e) => Some(e.offset),
            SrcSlot::Retired { offset } => Some(offset),
            _ => None,
        }
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

    /// Slot `s` takes an entry's bytes as they are: no encode. The bytes must
    /// already be valid: the compile-time blob only; anything from disk or USB
    /// goes through `store`.
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

    /// The scratch Sound, as the last load left it.
    pub(in crate::project) fn scratch_ref(&self) -> &Sound {
        &self.scratch
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

    /// Its `SourceRef` record's, if it has one.
    pub fn source_ref(&self) -> Option<SourceRef> {
        let mut found = None;
        let _ = self.each(|e| {
            if let Event::Record(ReadTag::Known(RecordTag::SourceRef), p) = e {
                found = SourceRef::from_bytes(p);
            }
            Ok(())
        });
        found
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
