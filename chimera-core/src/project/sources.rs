//! The pool's sources (ADR 0087): a `SourceRef` names one, the pool holds
//! its bytes, and who uses it is derived from the project when asked,
//! never stored.

use super::arena::{Cost, MAX_SOURCES};
use super::{PartId, PartSet, Project, SlotId, bounded_index};

bounded_index!(
    /// One of the pool's source slots, and its `SourceTable` row.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    SourceId < MAX_SOURCES
);

impl SourceId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Its bit in a `SourceTable` mask.
    pub const fn bit(self) -> u32 {
        1 << self.0
    }
}

const _: () = assert!(MAX_SOURCES <= u32::BITS as usize);

/// Its on-disk ref lives in `storage` (a card format type), named here too.
pub use crate::storage::SourceRef;

/// A source's head in the arena, before its held bytes.
pub const SOURCE_HEAD: usize = 16;

/// A pool source: its ref and how many of its bytes the arena holds. An
/// absent one (its card file missing) holds none and costs nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceEntry {
    pub r: SourceRef,
    pub held: u32,
    /// Its head's arena offset; the held bytes follow it.
    pub(super) offset: u32,
}

impl SourceEntry {
    pub fn absent(&self) -> bool {
        self.held == 0
    }

    pub fn cost(&self) -> Cost {
        source_cost(self.held as usize)
    }
}

/// What source `r` takes held whole; `None` when it could never fit.
pub(super) fn ref_cost(r: SourceRef) -> Option<Cost> {
    crate::storage::held_len(r)
        .filter(|&n| n <= super::arena::ARENA_BYTES)
        .map(source_cost)
}

/// What `held` bytes take: none when absent, else the head and the bytes.
pub(super) fn source_cost(held: usize) -> Cost {
    if held == 0 {
        Cost::default()
    } else {
        Cost::of(SOURCE_HEAD + held)
    }
}

/// Who resolves to a source: Parts, and pool Sounds a bit per `SlotId`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Users {
    pub parts: PartSet,
    pub sounds: [u64; 2],
}

impl Users {
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty() && self.sounds == [0; 2]
    }

    pub fn has_sound(&self, s: SlotId) -> bool {
        self.sounds[s.index() / 64] & 1 << (s.index() % 64) != 0
    }

    fn with_sound(mut self, s: SlotId) -> Users {
        self.sounds[s.index() / 64] |= 1 << (s.index() % 64);
        self
    }

    fn without_sound(mut self, s: SlotId) -> Users {
        self.sounds[s.index() / 64] &= !(1 << (s.index() % 64));
        self
    }
}

/// The Parts and pool Sounds whose `SourceRef` is source `s`'s: none for
/// an empty slot.
pub fn source_users(p: &Project, s: SourceId) -> Users {
    let Some(e) = p.pool.source(s) else {
        return Users::default();
    };
    let mut u = Users::default();
    for part in PartId::ALL {
        if p.part(part).sound.source == Some(e.r) {
            u.parts = u.parts.with(part);
        }
    }
    for slot in SlotId::ALL {
        if p.pool.get(slot).and_then(|x| x.source_ref()) == Some(e.r) {
            u = u.with_sound(slot);
        }
    }
    u
}

/// What a purge took: the bytes it gives back, now or when a note ends,
/// and how many of its sources were still sounding (retired, not cut).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Purged {
    pub freed: Cost,
    pub retired: u8,
}

/// The unused sources: how many, and what they hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Unused {
    pub count: u8,
    pub cost: Cost,
}

impl Project {
    /// The sources nothing uses, absent ones included.
    fn unused(&self) -> impl Iterator<Item = (SourceId, SourceEntry)> + '_ {
        SourceId::ALL.into_iter().filter_map(|s| {
            let e = self.pool.source(s)?;
            source_users(self, s).is_empty().then_some((s, e))
        })
    }

    /// PURGE UNUSED SOURCES' count, as it would run now: sources holding
    /// bytes only. An unused absent entry goes with them, but alone it is
    /// nothing to offer.
    pub fn unused_sources(&self) -> Unused {
        self.unused()
            .filter(|(_, e)| !e.absent())
            .fold(Unused::default(), |u, (_, e)| Unused {
                count: u.count + 1,
                cost: u.cost + e.cost(),
            })
    }

    /// Every source nothing uses, recounted now (ADR 0087). A source in
    /// `sounding` is retired: its bytes stay pending until its note ends.
    /// The card is never touched. With none, nothing moves.
    pub fn purge_unused_sources(&mut self, sounding: u32) -> Purged {
        let mut gone = [None; MAX_SOURCES];
        for (i, (s, e)) in self.unused().enumerate() {
            gone[i] = Some((s, e));
        }
        if gone[0].is_none() {
            return Purged::default();
        }
        self.bump();
        let mut out = Purged::default();
        for (s, e) in gone.into_iter().flatten() {
            out.freed = out.freed + e.cost();
            if sounding & s.bit() != 0 && !e.absent() {
                out.retired += 1;
            }
            self.pool.source_free(s);
        }
        out
    }

    /// Clearing slot `s` would leave its source with no user: that source's
    /// cost. `None` when the slot holds no ref, or another still uses it.
    pub fn orphaned_by_clear(&self, s: SlotId) -> Option<Cost> {
        let r = self.pool.get(s)?.source_ref()?;
        let id = self.pool.find_source(r)?;
        if !source_users(self, id).without_sound(s).is_empty() {
            return None;
        }
        self.pool.source(id).map(|e| e.cost())
    }

    /// Stores source `r`, its `held` bytes copied into the arena (Task 9's
    /// loader; tests). An identical source already held is shared.
    pub fn source_store(&mut self, r: SourceRef, held: &[u8]) -> Result<SourceId, super::PoolFull> {
        self.bump();
        self.pool.source_store(r, held)
    }

    /// Source `r` as an absent entry, costing nothing.
    pub fn source_absent(&mut self, r: SourceRef) -> Option<SourceId> {
        self.bump();
        self.pool.source_absent(r)
    }

    /// One UI pass of the sources' gated compaction (ADR 0087), waiting
    /// by the gate clock: true when a hole closed. Bumps only then, so a
    /// status cache keyed on `rev` isn't redone each frame.
    pub fn sources_pass(&mut self) -> bool {
        let moved = self.pool.source_pass_timed();
        if moved {
            self.bump();
        }
        moved
    }

    /// `sources_pass`, waiting for the audio while `within` says so. Tests
    /// only.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn sources_pass_with(&mut self, within: impl FnMut() -> bool) -> bool {
        let moved = self.pool.source_pass(within);
        if moved {
            self.bump();
        }
        moved
    }

    /// The UI loop's clock in ms, which a gate waits by
    /// (`GATE_ACK_TIMEOUT_MS`).
    pub fn set_gate_clock(&mut self, now_ms: fn() -> u32) {
        self.pool.set_clock(now_ms);
    }
}
