//! The pool arena (ADR 0087): one 160 KiB block, sources packed up from 0,
//! Sounds packed down from the top, free space the gap between them.
//! Sounds compact at once; a freed source is a pending hole until nothing
//! that would move is sounding.

use core::fmt::Write as _;
use core::ptr::NonNull;

use crate::ui::fmt::FmtBuf;

pub const ARENA_BYTES: usize = 160 * 1024;
pub const GRANULE: usize = 16;
pub const MAX_SOURCES: usize = 32;

/// Arena bytes, a whole number of granules: what the meter adds up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cost(u32);

impl Cost {
    /// `len` bytes rounded up to the granule.
    pub const fn of(len: usize) -> Cost {
        let g = len.div_ceil(GRANULE) * GRANULE;
        Cost(if g > u32::MAX as usize {
            u32::MAX - (GRANULE as u32 - 1)
        } else {
            g as u32
        })
    }

    pub const fn bytes(self) -> u32 {
        self.0
    }

    /// KiB rounded up: `0.3K`, `9.9K` under 10, then `10K`, `41K`.
    pub fn kib_label(self) -> FmtBuf {
        let b = self.0 as u64;
        let tenths = (b * 10).div_ceil(1024);
        let mut f = FmtBuf::new();
        let _ = if tenths < 100 {
            write!(f, "{}.{}K", tenths / 10, tenths % 10)
        } else {
            write!(f, "{}K", b.div_ceil(1024))
        };
        f
    }
}

/// What a free or a compaction moved: every entry on the moving side of
/// `from` moved `by` bytes toward its own end. For `sound_free`, Sounds at
/// offsets below `from` moved up; for `source_compact`, sources above
/// `from` moved down. The caller rewrites its index from this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shift {
    pub from: u32,
    pub by: u32,
}

#[derive(Clone, Copy)]
struct Src {
    offset: u32,
    cost: u32,
    pending: bool,
}

const NO_SRC: Src = Src {
    offset: 0,
    cost: 0,
    pending: false,
};

/// Why a source didn't get a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The gap is smaller than the source.
    Full,
    /// All `MAX_SOURCES` rows are taken, by pending holes a voice still
    /// reads: they free themselves when the note ends.
    RowsBusy,
}

/// A pending source hole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hole {
    pub offset: u32,
    pub cost: Cost,
}

/// Proof that ADR 0087's gate is closed and acked, carrying the re-check
/// read after the ack: `ready(offset)` is true iff no voice is on the
/// source at that offset. Only the gate mints one (Task 8); until then,
/// tests assume it.
pub struct GateAcked<F: Fn(u32) -> bool> {
    ready: F,
}

impl<F: Fn(u32) -> bool> GateAcked<F> {
    /// Tests only: Task 8's gate owns the real constructor.
    #[cfg(any(test, feature = "test-support"))]
    pub fn assumed(ready: F) -> Self {
        GateAcked { ready }
    }
}

/// The only writer of the arena's bytes. The audio's view (Task 8's
/// `SourceReader`) reads through its own pointer, so this holds a raw one
/// rather than a `&mut` that would claim the bytes exclusively.
pub struct ArenaWriter {
    base: NonNull<u8>,
    /// Sources fill `[0, src_end)`, in offset order.
    src_end: u32,
    /// Sounds fill `[snd_start, ARENA_BYTES)`.
    snd_start: u32,
    srcs: [Src; MAX_SOURCES],
    n_srcs: usize,
}

// SAFETY: built from a `&'static mut [u8]`, so it owns its bytes as that
// borrow did, and a `&'static mut [u8]` is `Send`.
unsafe impl Send for ArenaWriter {}

impl ArenaWriter {
    /// Takes the arena's bytes for good: the AXI static on the chip, a
    /// leaked box in tests and on the desktop.
    pub fn new(bytes: &'static mut [u8]) -> ArenaWriter {
        assert_eq!(bytes.len(), ARENA_BYTES);
        ArenaWriter {
            base: NonNull::from(bytes).cast(),
            src_end: 0,
            snd_start: ARENA_BYTES as u32,
            srcs: [NO_SRC; MAX_SOURCES],
            n_srcs: 0,
        }
    }

    /// Bytes held by Sounds and sources, pending holes included.
    pub fn used(&self) -> Cost {
        Cost(ARENA_BYTES as u32 - self.gap().0)
    }

    /// Bytes in pending source holes: used, but not yet free.
    pub fn pending(&self) -> Cost {
        Cost(
            self.live_srcs()
                .iter()
                .filter(|s| s.pending)
                .map(|s| s.cost)
                .sum(),
        )
    }

    /// The free space between the ends: an add fits iff its cost ≤ this.
    pub fn gap(&self) -> Cost {
        Cost(self.snd_start - self.src_end)
    }

    /// A Sound of `len` bytes below the others, or `None` if it won't fit.
    pub fn sound_alloc(&mut self, len: usize) -> Option<u32> {
        let c = Cost::of(len);
        if c > self.gap() {
            return None;
        }
        self.snd_start -= c.0;
        Some(self.snd_start)
    }

    /// Frees the Sound at `offset` and closes its hole at once: the audio
    /// never reads Sound bytes.
    pub fn sound_free(&mut self, offset: u32, len: usize) -> Shift {
        let by = Cost::of(len).0;
        let (lo, end) = (self.snd_start as usize, offset as usize);
        assert!(lo <= end && end + by as usize <= ARENA_BYTES && end % GRANULE == 0);
        self.shift(lo, lo + by as usize, end - lo);
        self.snd_start += by;
        Shift { from: offset, by }
    }

    /// Where the next source goes: `len` bytes at the bottom of the gap to
    /// stage and verify before `source_alloc` publishes them.
    pub fn source_stage(&mut self, len: usize) -> Result<&mut [u8], Refused> {
        self.source_fits(Cost::of(len))?;
        Ok(self.bytes_mut(self.src_end, len))
    }

    /// Takes the `len` bytes staged at the bottom of the gap as a source.
    pub fn source_alloc(&mut self, len: usize) -> Result<u32, Refused> {
        let c = Cost::of(len);
        self.source_fits(c)?;
        let offset = self.src_end;
        self.srcs[self.n_srcs] = Src {
            offset,
            cost: c.0,
            pending: false,
        };
        self.n_srcs += 1;
        self.src_end += c.0;
        Ok(offset)
    }

    fn source_fits(&self, c: Cost) -> Result<(), Refused> {
        if c > self.gap() {
            Err(Refused::Full)
        } else if self.n_srcs == MAX_SOURCES {
            Err(Refused::RowsBusy)
        } else {
            Ok(())
        }
    }

    /// Marks the source at `offset` a hole. Its bytes stay where they are,
    /// still readable by a ringing note, until `source_compact` closes it.
    pub fn source_free_pending(&mut self, offset: u32, len: usize) {
        let cost = Cost::of(len).0;
        let s = self.srcs[..self.n_srcs]
            .iter_mut()
            .find(|s| s.offset == offset)
            .expect("a source at that offset");
        assert!(s.cost == cost && !s.pending);
        s.pending = true;
    }

    /// The hole `source_compact` closes next: the lowest. The sources at
    /// and above its offset are ADR 0087's `above` mask.
    pub fn next_compaction(&self) -> Option<Hole> {
        self.live_srcs().iter().find(|s| s.pending).map(|s| Hole {
            offset: s.offset,
            cost: Cost(s.cost),
        })
    }

    /// Closes `next_compaction()`'s hole, sliding every source above it
    /// down, if the gate's re-check finds the hole and every source above
    /// it ready (no voice on it). `None` leaves everything where it was.
    pub fn source_compact<F: Fn(u32) -> bool>(&mut self, gate: GateAcked<F>) -> Option<Shift> {
        let n = self.n_srcs;
        let i = self.live_srcs().iter().position(|s| s.pending)?;
        if !self.srcs[i..n].iter().all(|s| (gate.ready)(s.offset)) {
            return None;
        }
        let Src {
            offset: from,
            cost: by,
            ..
        } = self.srcs[i];
        let lo = (from + by) as usize;
        self.shift(lo, from as usize, self.src_end as usize - lo);
        self.srcs.copy_within(i + 1..n, i);
        self.n_srcs -= 1;
        for s in &mut self.srcs[i..self.n_srcs] {
            s.offset -= by;
        }
        self.src_end -= by;
        Some(Shift { from, by })
    }

    /// Any entry's bytes, or the gap's. Reads only, as the audio's are.
    pub fn bytes(&self, offset: u32, len: usize) -> &[u8] {
        let o = offset as usize;
        assert!(o + len <= ARENA_BYTES);
        // SAFETY: `[o, o+len)` is inside the bytes `new` took for 'static.
        // Shared and only this range: the audio reads sources too, and no
        // one writes while `&self` is lent, since the UI holds the only
        // writer and published sources never take writes (`bytes_mut`).
        unsafe { core::slice::from_raw_parts(self.base.as_ptr().add(o), len) }
    }

    /// UI only: the gap (staging an add) or Sounds, never a published
    /// source, whose bytes the audio may be reading.
    pub fn bytes_mut(&mut self, offset: u32, len: usize) -> &mut [u8] {
        let o = offset as usize;
        assert!(o >= self.src_end as usize && o + len <= ARENA_BYTES);
        // SAFETY: `[o, o+len)` is inside the arena and at or above
        // `src_end`: the gap and the Sounds, which no source row points
        // into, so the audio never reads them. `&mut self` keeps every
        // other Rust reference out for the borrow's life.
        unsafe { core::slice::from_raw_parts_mut(self.base.as_ptr().add(o), len) }
    }

    /// Moves `n` bytes from `src` to `dst`, overlapping allowed.
    fn shift(&mut self, src: usize, dst: usize, n: usize) {
        assert!(src.max(dst) + n <= ARENA_BYTES);
        // SAFETY: both ranges are inside the arena. Raw pointers, no
        // reference, so nothing claims the bytes the audio may read
        // elsewhere. A Sound move touches only `[snd_start, ARENA)`, which
        // no row points into. A source move runs only on a `GateAcked`:
        // the gate is closed and acked and no voice is on any source it
        // moves (ADR 0087).
        unsafe {
            let p = self.base.as_ptr();
            core::ptr::copy(p.add(src), p.add(dst), n);
        }
    }

    fn live_srcs(&self) -> &[Src] {
        &self.srcs[..self.n_srcs]
    }
}
