//! The audio's view of the pool's sources (ADR 0087): a row per source,
//! the gate the UI closes while it moves source bytes, and the mask of
//! sources the audio's voices hold. Atomics only: the UI writes rows and
//! the gate, the audio writes the ack, the nack and `sounding`.
//!
//! One reader: `reader` hands out the table's only `AudioToken`, and every
//! block's `GateView` borrows it, so blocks run one after another on one
//! owner.
//!
//! Ordering:
//! - **publish:** `base` and `rate` `Relaxed`, then `held` `Release`; a
//!   reader loads `held` `Acquire` first. The id itself reaches the audio
//!   later, through the triple buffer's own `Release`.
//! - **close:** the moving mask `Relaxed`, then the epoch `Release`. The
//!   audio's `begin_block` loads the epoch `Acquire` before the mask, so a
//!   block that answers an epoch saw that epoch's mask.
//! - **ack / nack:** `Release` by the audio after its loads. The UI's
//!   `Acquire` of the ack orders every earlier block's reads before the
//!   move. A block whose voices hold a moving source nacks instead, and
//!   the UI opens without moving.
//! - **open:** rows rewritten `Relaxed`, then the gate `Release`; the
//!   audio's next `begin_block` loads it `Acquire` and so sees the rows.

#[cfg(not(loom))]
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering, fence};
#[cfg(loom)]
use loom::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering, fence};

use crate::project::SourceId;
use crate::project::arena::{ARENA_BYTES, MAX_SOURCES};

/// How long a gate waits for the audio's answer: two blocks and change.
/// Every block answers, so only a stalled audio runs it out.
pub const GATE_ACK_TIMEOUT_MS: u32 = 6;

/// Where a source's held bytes sit in the arena, and its rate. `held` 0
/// is an empty row: nothing to read.
pub struct SourceRow {
    base: AtomicU32,
    held: AtomicU32,
    rate: AtomicU32,
}

impl SourceRow {
    #[cfg(not(loom))]
    const fn empty() -> SourceRow {
        SourceRow {
            base: AtomicU32::new(0),
            held: AtomicU32::new(0),
            rate: AtomicU32::new(0),
        }
    }

    #[cfg(loom)]
    fn empty() -> SourceRow {
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
    /// Moves on every close: what the audio answers.
    epoch: AtomicU32,
    ack: AtomicU32,
    nack: AtomicU32,
    /// Written by the audio once a block: bit n set while a voice holds
    /// source n, one waiting at a closed gate included.
    sounding: AtomicU32,
    /// Audio blocks begun: each one after the last block's `sounding`.
    blocks: AtomicU32,
    /// The arena's first byte: null until `attach`.
    arena: AtomicPtr<u8>,
    /// The `AudioToken` is out: someone may read.
    reader: AtomicBool,
    /// The audio's callback period in µs, as its shell last said: 0 until
    /// it does.
    period_us: AtomicU32,
}

/// The chip's and the desktop's one table, beside the one arena.
#[cfg(not(loom))]
pub static SOURCE_TABLE: SourceTable = SourceTable::new();

#[cfg(not(loom))]
impl Default for SourceTable {
    fn default() -> Self {
        Self::new()
    }
}

/// What the UI's wait on a closed gate came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The audio began a block under the gate, no voice on a moving id.
    Acked,
    /// A voice holds a moving id: open, and try again later.
    Nacked,
    /// No answer in time: the audio is stalled.
    TimedOut,
}

impl SourceTable {
    #[cfg(not(loom))]
    pub const fn new() -> Self {
        SourceTable {
            rows: [const { SourceRow::empty() }; MAX_SOURCES],
            gate: AtomicU32::new(0),
            epoch: AtomicU32::new(0),
            ack: AtomicU32::new(0),
            nack: AtomicU32::new(0),
            sounding: AtomicU32::new(0),
            blocks: AtomicU32::new(0),
            arena: AtomicPtr::new(core::ptr::null_mut()),
            reader: AtomicBool::new(false),
            period_us: AtomicU32::new(0),
        }
    }

    /// loom's atomics aren't `const`.
    #[cfg(loom)]
    pub fn new() -> Self {
        SourceTable {
            rows: core::array::from_fn(|_| SourceRow::empty()),
            gate: AtomicU32::new(0),
            epoch: AtomicU32::new(0),
            ack: AtomicU32::new(0),
            nack: AtomicU32::new(0),
            sounding: AtomicU32::new(0),
            blocks: AtomicU32::new(0),
            arena: AtomicPtr::new(core::ptr::null_mut()),
            reader: AtomicBool::new(false),
            period_us: AtomicU32::new(0),
        }
    }

    /// The table's one `AudioToken`: `None` once taken.
    ///
    /// Take it before any source pass can run (the chip and the desktop
    /// take it as their audio engine is built, before the UI loop). A pass
    /// that finds no reader moves without waiting, since nothing can read;
    /// a reader taken in the middle of that pass could read a moving byte.
    pub fn reader(&'static self) -> Option<AudioToken> {
        let taken = self.reader.swap(true, Ordering::AcqRel);
        // Orders the flag before every later gate load, against a pass's
        // `has_reader` and its close on the UI side.
        fence(Ordering::SeqCst);
        (!taken).then_some(AudioToken {
            table: self,
            answered: (0, true),
            acked: 0,
        })
    }

    /// Binds the arena the rows' bases are offsets into, once: a second
    /// arena would leave the first's rows pointing at the second's bytes.
    ///
    /// # Panics
    /// When already attached: one table per arena.
    pub(crate) fn attach(&self, base: *mut u8) {
        let was = self.arena.compare_exchange(
            core::ptr::null_mut(),
            base,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        assert!(was.is_ok(), "a source table serves one arena");
        self.gate.store(0, Ordering::Release);
    }

    /// UI: source `id` reads `held` bytes from arena offset `base`. The
    /// row must be empty, or under an acked gate that moves `id`.
    pub(crate) fn publish(&self, id: SourceId, base: u32, held: u32, rate: u16) {
        let r = &self.rows[id.index()];
        r.base.store(base, Ordering::Relaxed);
        r.rate.store(rate.into(), Ordering::Relaxed);
        r.held.store(held, Ordering::Release);
    }

    /// UI: `id`'s row reads nothing. Under an acked gate that moves `id`.
    pub(crate) fn clear(&self, id: SourceId) {
        self.rows[id.index()].held.store(0, Ordering::Relaxed);
    }

    /// UI: `id`'s base, under an acked gate that moves `id`.
    pub(crate) fn rebase(&self, id: SourceId, base: u32) {
        self.rows[id.index()].base.store(base, Ordering::Relaxed);
    }

    /// UI: closes the gate on the ids in `moving`; the epoch to wait for.
    pub(crate) fn close_gate(&self, moving: u32) -> u32 {
        self.gate.store(moving, Ordering::Relaxed);
        self.epoch.fetch_add(1, Ordering::Release).wrapping_add(1)
    }

    /// UI: waits for the audio's answer to the close that returned
    /// `epoch`, while `within` says so. No reader: nothing can read, so
    /// acked at once.
    pub(crate) fn wait(&self, epoch: u32, mut within: impl FnMut() -> bool) -> Answer {
        if !self.has_reader() {
            return Answer::Acked;
        }
        loop {
            if self.ack.load(Ordering::Acquire) == epoch {
                return Answer::Acked;
            }
            if self.nack.load(Ordering::Acquire) == epoch {
                return Answer::Nacked;
            }
            if !within() {
                return Answer::TimedOut;
            }
            core::hint::spin_loop();
        }
    }

    /// UI: every source reads again, from the rows as rewritten.
    pub(crate) fn open_gate(&self) {
        self.gate.store(0, Ordering::Release);
    }

    /// UI: the sources a voice held at the end of the last block.
    pub fn sounding(&self) -> u32 {
        self.sounding.load(Ordering::Acquire)
    }

    /// UI: the audio's callback period in ms, rounded up; 0 if unknown.
    pub fn period_ms(&self) -> u32 {
        self.period_us.load(Ordering::Relaxed).div_ceil(1000)
    }

    /// The `AudioToken` is out: something may read.
    pub fn has_reader(&self) -> bool {
        self.reader.load(Ordering::Acquire)
    }

    /// UI: audio blocks begun. Read before `sounding`, a count `n` means
    /// `sounding` is from block `n - 1`'s end or later.
    pub fn blocks(&self) -> u32 {
        self.blocks.load(Ordering::Acquire)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn publish_for_test(&self, id: SourceId, base: u32, held: u32, rate: u16) {
        self.publish(id, base, held, rate)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn close_gate_for_test(&self, moving: u32) -> u32 {
        self.close_gate(moving)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn open_gate_for_test(&self) {
        self.open_gate()
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn rebase_for_test(&self, id: SourceId, base: u32) {
        self.rebase(id, base)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn clear_for_test(&self, id: SourceId) {
        self.clear(id)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn wait_for_test(&self, epoch: u32, within: impl FnMut() -> bool) -> Answer {
        self.wait(epoch, within)
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn attach_for_test(&self, base: *mut u8) {
        self.attach(base)
    }
}

/// The audio's side of a table: only `SourceTable::reader` makes one, once.
pub struct AudioToken {
    table: &'static SourceTable,
    /// The last epoch this side answered, and whether it acked: an answer
    /// holds for every block of its epoch.
    answered: (u32, bool),
    /// The mask that ack covered: later views of the epoch read `gate &
    /// acked`. Cleared when a view sees the gate open.
    acked: u32,
}

impl AudioToken {
    /// At each block's start: answers a new close, and the block's view.
    /// A block whose voices (as last stored) hold a moving id nacks and
    /// reads as if the gate were open: the UI won't move without an ack.
    pub fn begin_block(&mut self) -> GateView<'_> {
        let t = self.table;
        t.blocks.fetch_add(1, Ordering::Release);
        let e = t.epoch.load(Ordering::Acquire);
        let gate = t.gate.load(Ordering::Acquire);
        if self.answered.0 != e {
            let held = t.sounding.load(Ordering::Relaxed);
            let ack = gate & held == 0;
            let to = if ack { &t.ack } else { &t.nack };
            to.store(e, Ordering::Release);
            self.answered = (e, ack);
            self.acked = if ack { gate } else { 0 };
        }
        if gate == 0 {
            self.acked = 0;
        }
        let moving = gate & self.acked;
        GateView {
            table: t,
            moving,
            arena: t.arena.load(Ordering::Acquire),
            _token: core::marker::PhantomData,
        }
    }

    /// Once a block after its voices: the sources they hold, one waiting
    /// at a closed gate included.
    pub fn store_sounding(&mut self, mask: u32) {
        self.table.sounding.store(mask, Ordering::Release);
    }

    /// The shell's callback period, in µs: how long the UI's waits on the
    /// audio must allow (ADR 0099).
    pub fn set_period_us(&mut self, us: u32) {
        self.table.period_us.store(us, Ordering::Relaxed);
    }
}

/// One block's reads: the sources a closed, acked gate moves read nothing.
/// Not `Send` (its raw pointer): a view stays on the token owner's thread
/// for its block.
///
/// ```compile_fail
/// fn send<T: Send>(_: T) {}
/// fn f(t: &mut chimera_core::source_table::AudioToken) {
///     send(t.begin_block());
/// }
/// ```
pub struct GateView<'b> {
    table: &'static SourceTable,
    moving: u32,
    arena: *const u8,
    _token: core::marker::PhantomData<&'b mut AudioToken>,
}

impl GateView<'_> {
    /// Source `id`'s held bytes; `None` while the gate moves it, or while
    /// its row is empty.
    ///
    /// The voice's contract: on `None` with `moving(id)`, it waits and
    /// keeps its bit in `store_sounding`. On `None` with the gate open,
    /// its row is gone (retired and compacted): it ends, and drops its
    /// bit from the next `store_sounding`, which frees the id for reuse.
    pub fn bytes(&self, id: SourceId) -> Option<&[u8]> {
        if self.moving & id.bit() != 0 {
            return None;
        }
        let r = &self.table.rows[id.index()];
        let held = r.held.load(Ordering::Acquire) as usize;
        let base = r.base.load(Ordering::Relaxed) as usize;
        if held == 0 || self.arena.is_null() || base + held > ARENA_BYTES {
            return None;
        }
        // SAFETY (the `SourceReader` argument, ADR 0087): `arena` is the
        // `&'static mut [u8; ARENA_BYTES]` `ArenaWriter::new` attached
        // (once), and `[base, base+held)` is inside it. The UI writes a
        // live row's bytes only after it reads an ack for an epoch whose
        // mask has `id`, and before it opens the gate. Views come one at
        // a time from the table's one `AudioToken` (each borrows it, and
        // can't leave its thread), so every view before the acking one
        // ended before that ack was stored; the ack's `Release` and the
        // UI's `Acquire` order their reads before the move. The acking
        // view and every later one of its epoch carry the acked mask
        // (`gate & acked`) while the gate stays closed, and returned
        // above; a view that loads the gate open loads it `Acquire`, after
        // the move and the rebased rows. A nacking view carries none, but the UI never
        // moves on a nack. The writer's `bytes_mut` never reaches below
        // the sources' end, where live rows point.
        Some(unsafe { core::slice::from_raw_parts(self.arena.add(base), held) })
    }

    /// The gate moves `id` this block: a note-on on it waits.
    pub fn moving(&self, id: SourceId) -> bool {
        self.moving & id.bit() != 0
    }

    /// Source `id`'s rate in Hz, 0 for an empty row.
    pub fn rate(&self, id: SourceId) -> u32 {
        self.table.rows[id.index()].rate.load(Ordering::Relaxed)
    }
}
