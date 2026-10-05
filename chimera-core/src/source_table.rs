//! The audio's view of the pool's sources (ADR 0087): a row per source,
//! the gate the UI closes while it moves source bytes, and the mask of
//! sources a voice reads. Atomics only: the UI writes rows and the gate,
//! the audio writes the ack and `sounding`.
//!
//! Ordering:
//! - **publish:** a row's fields `Relaxed`, then `fence(Release)`. The id
//!   reaches the audio later, through the triple buffer's own `Release`.
//! - **close:** the moving mask `Relaxed`, then the epoch `Release`. The
//!   audio's `begin_block` loads the epoch `Acquire` before the mask, so a
//!   block that acks an epoch saw that epoch's mask.
//! - **ack:** `Release` by the audio after its loads; the UI's `Acquire`
//!   then sees every `sounding` the audio stored before it.
//! - **open:** rows rewritten `Relaxed`, then the gate `Release`; the
//!   audio's next `begin_block` loads it `Acquire` and so sees the rows.

use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering, fence};

use crate::project::SourceId;
use crate::project::arena::{ARENA_BYTES, MAX_SOURCES};

/// Where a source's held bytes sit in the arena, and its rate. `held` 0
/// is an empty row: nothing to read.
pub struct SourceRow {
    base: AtomicU32,
    held: AtomicU32,
    rate: AtomicU32,
}

impl SourceRow {
    const fn empty() -> SourceRow {
        SourceRow {
            base: AtomicU32::new(0),
            held: AtomicU32::new(0),
            rate: AtomicU32::new(0),
        }
    }
}

pub struct SourceTable {
    rows: [SourceRow; MAX_SOURCES],
    /// The ids a closed gate moves, a bit each; 0 while open.
    gate: AtomicU32,
    /// Moves on every close: what the audio acks.
    epoch: AtomicU32,
    ack: AtomicU32,
    /// Written by the audio once a block: bit n set while a voice holds
    /// source n.
    sounding: AtomicU32,
    /// The arena's first byte: null until `attach`.
    arena: AtomicPtr<u8>,
}

/// The chip's and the desktop's one table, beside the one arena.
pub static SOURCE_TABLE: SourceTable = SourceTable::new();

impl Default for SourceTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceTable {
    pub const fn new() -> Self {
        SourceTable {
            rows: [const { SourceRow::empty() }; MAX_SOURCES],
            gate: AtomicU32::new(0),
            epoch: AtomicU32::new(0),
            ack: AtomicU32::new(0),
            sounding: AtomicU32::new(0),
            arena: AtomicPtr::new(core::ptr::null_mut()),
        }
    }

    /// The arena the rows' bases are offsets into: `ArenaWriter::new`'s.
    pub(crate) fn attach(&self, base: *mut u8) {
        self.arena.store(base, Ordering::Release);
    }

    /// UI: source `id` reads `held` bytes from arena offset `base`. The
    /// row must be empty or under a closed gate that moves `id`.
    pub fn publish(&self, id: SourceId, base: u32, held: u32, rate: u16) {
        let r = &self.rows[id.index()];
        r.base.store(base, Ordering::Relaxed);
        r.held.store(held, Ordering::Relaxed);
        r.rate.store(rate.into(), Ordering::Relaxed);
        fence(Ordering::Release);
    }

    /// UI: `id`'s row reads nothing. Under a closed gate that moves `id`.
    pub(crate) fn clear(&self, id: SourceId) {
        self.rows[id.index()].held.store(0, Ordering::Relaxed);
    }

    /// UI: `id`'s base, under a closed gate that moves `id`.
    pub(crate) fn rebase(&self, id: SourceId, base: u32) {
        self.rows[id.index()].base.store(base, Ordering::Relaxed);
    }

    /// UI: closes the gate on the ids in `moving`; the epoch to wait for.
    pub fn close_gate(&self, moving: u32) -> u32 {
        self.gate.store(moving, Ordering::Relaxed);
        self.epoch.fetch_add(1, Ordering::Release).wrapping_add(1)
    }

    /// UI: the audio began a block after the close that returned `epoch`.
    pub fn gate_acked(&self, epoch: u32) -> bool {
        self.ack.load(Ordering::Acquire) == epoch
    }

    /// UI: every source reads again, from the rows as rewritten.
    pub fn open_gate(&self) {
        self.gate.store(0, Ordering::Release);
    }

    /// UI: the sources a voice held at the end of the last block.
    pub fn sounding(&self) -> u32 {
        self.sounding.load(Ordering::Acquire)
    }

    /// Audio, once a block after its voices: the sources they hold, a
    /// voice waiting on a closed gate included.
    pub fn store_sounding(&self, mask: u32) {
        self.sounding.store(mask, Ordering::Release);
    }

    /// Audio, at each block's start: acks the gate, and the block's view.
    pub fn begin_block(&self) -> GateView<'_> {
        let e = self.epoch.load(Ordering::Acquire);
        let moving = self.gate.load(Ordering::Acquire);
        self.ack.store(e, Ordering::Release);
        GateView {
            table: self,
            moving,
        }
    }
}

/// One block's reads: the sources a closed gate moves read nothing.
pub struct GateView<'a> {
    table: &'a SourceTable,
    moving: u32,
}

impl GateView<'_> {
    /// Source `id`'s held bytes; `None` while the gate moves it, or while
    /// its row is empty.
    pub fn bytes(&self, id: SourceId) -> Option<&[u8]> {
        if self.moving & id.bit() != 0 {
            return None;
        }
        let r = &self.table.rows[id.index()];
        let held = r.held.load(Ordering::Relaxed) as usize;
        let base = r.base.load(Ordering::Relaxed) as usize;
        let arena = self.table.arena.load(Ordering::Acquire);
        if held == 0 || arena.is_null() || base + held > ARENA_BYTES {
            return None;
        }
        // SAFETY (the `SourceReader` argument, ADR 0087): `arena` is the
        // `&'static mut [u8; ARENA_BYTES]` `ArenaWriter::new` took, and
        // `[base, base+held)` is inside it. The row is live (`held > 0`),
        // and the UI writes a live row's bytes only under a closed, acked
        // gate that moves that row's id: then `moving` has its bit and we
        // returned above. The ack is stored after this block's `moving`
        // was read, so no move starts until a later block. The writer's
        // `bytes_mut` never reaches below the sources' end, where live rows
        // point, so no `&mut` overlaps this `&`.
        Some(unsafe { core::slice::from_raw_parts(arena.add(base), held) })
    }

    /// Source `id`'s rate in Hz, 0 for an empty row.
    pub fn rate(&self, id: SourceId) -> u32 {
        self.table.rows[id.index()].rate.load(Ordering::Relaxed)
    }
}
