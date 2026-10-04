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
        self.all_mut().copy_within(lo..end, lo + by as usize);
        self.snd_start += by;
        Shift { from: offset, by }
    }

    /// A source of `len` bytes above the others, or `None` if it won't fit
    /// or every source row is taken (pending holes hold theirs).
    pub fn source_alloc(&mut self, len: usize) -> Option<u32> {
        let c = Cost::of(len);
        if c > self.gap() || self.n_srcs == MAX_SOURCES {
            return None;
        }
        let offset = self.src_end;
        self.srcs[self.n_srcs] = Src {
            offset,
            cost: c.0,
            pending: false,
        };
        self.n_srcs += 1;
        self.src_end += c.0;
        Some(offset)
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

    /// Closes one pending hole by sliding every source above it down, if
    /// the hole and everything above it is `ready` (nothing reads it). The
    /// lowest hole that qualifies goes; `None` if none does. `ready` is
    /// asked of each source's offset, as its bytes are now, and the caller
    /// has closed and acknowledged ADR 0087's gate before it answers.
    pub fn source_compact(&mut self, ready: impl Fn(u32) -> bool) -> Option<Shift> {
        let n = self.n_srcs;
        // The highest source that isn't ready: nothing at or below it moves.
        let floor = (0..n)
            .rev()
            .find(|&i| !ready(self.srcs[i].offset))
            .map_or(0, |i| i + 1);
        let i = (floor..n).find(|&i| self.srcs[i].pending)?;
        let Src {
            offset: from,
            cost: by,
            ..
        } = self.srcs[i];
        let (lo, hi) = ((from + by) as usize, self.src_end as usize);
        self.all_mut().copy_within(lo..hi, from as usize);
        self.srcs.copy_within(i + 1..n, i);
        self.n_srcs -= 1;
        for s in &mut self.srcs[i..self.n_srcs] {
            s.offset -= by;
        }
        self.src_end -= by;
        Some(Shift { from, by })
    }

    pub fn bytes(&self, offset: u32, len: usize) -> &[u8] {
        &self.all()[offset as usize..][..len]
    }

    /// UI only: staging an add into the gap, or an entry's own bytes.
    pub fn bytes_mut(&mut self, offset: u32, len: usize) -> &mut [u8] {
        &mut self.all_mut()[offset as usize..][..len]
    }

    fn live_srcs(&self) -> &[Src] {
        &self.srcs[..self.n_srcs]
    }

    fn all(&self) -> &[u8] {
        // SAFETY: `base` is the start of the `ARENA_BYTES` taken in `new`
        // for 'static. The audio reads a source's bytes only through rows
        // the source table publishes; the UI writes them only through
        // `&mut self`, and moves them only under ADR 0087's gate (Task 8),
        // so no write overlaps this shared view.
        unsafe { core::slice::from_raw_parts(self.base.as_ptr(), ARENA_BYTES) }
    }

    fn all_mut(&mut self) -> &mut [u8] {
        // SAFETY: as `all`, and `&mut self` makes this the only Rust
        // reference into the bytes. The audio may read a live source's
        // bytes concurrently: the UI writes a published source's bytes only
        // inside ADR 0087's gate (Task 8), after the post-ack re-check
        // shows no voice on them, and writes the gap freely, which no row
        // points into.
        unsafe { core::slice::from_raw_parts_mut(self.base.as_ptr(), ARENA_BYTES) }
    }
}
