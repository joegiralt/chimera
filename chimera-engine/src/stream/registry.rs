//! The stream registry (slice 1 § 5.5): a row per streamed source, its
//! `SourceId` and `StreamFile`. The UI writes rows; the card IRQ and the
//! audio read them.
//!
//! Ordering: a row's tag holds its id + 1 (0 free). `insert` claims a free
//! row by CAS, writes its file, then stores the tag `Release`; `get` loads
//! the tag `Acquire` before it reads the file. A row is rewritten only
//! after `clear`, whose caller vouches no reader but the card's hold
//! still holds it.
//!
//! The card IRQ's read spans many entries, so it holds its row (#597), as
//! the audio's `sounding` mask holds a source (ADR 0087): `clear` leaves a
//! held row, and the pool's drain tries again on its next pass. A row
//! `clear_all` leaves so is the last card's: stale, found by no `get` or
//! `hold` again, so no read after the one in flight lands its blocks on
//! the new card. Rows are inserted and cleared from the UI's thread alone.

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use chimera_hal::store::FileName;

use super::layout::StreamFile;
use super::ring::AtomicTag;
use crate::project::SourceId;

/// Rows: one a ring, so one a stream.
pub const STREAM_ROWS: usize = 8;

const FREE: u8 = 0;
/// Claimed by `insert`, its file being written.
const WRITING: u8 = u8::MAX;

// Every id + 1 is a tag: neither `FREE` nor `WRITING`.
const _: () = assert!(SourceId::ALL.len() < WRITING as usize);

/// Why `insert` took no row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowRefused {
    /// Every row taken: a ninth streamed source refuses to load.
    Full,
    /// A row holds the id already: one row an id.
    Taken,
}

struct Row {
    tag: AtomicU8,
    /// The card condemned its file whole (#609): `FILE DAMAGED`.
    damaged: AtomicBool,
    /// Held across `clear_all`: another card's file, for no reader.
    stale: AtomicBool,
    file: UnsafeCell<MaybeUninit<StreamFile>>,
}

/// Which card the rows' files are on: it moves when every row is freed for
/// another card's (`clear_all`), and the card interrupt forgets what it
/// blamed on the last one's files (`serve::Rot`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CardEpoch(u32);

impl CardEpoch {
    /// A zeroed registry's: the card it boots with.
    pub const FIRST: CardEpoch = CardEpoch(0);

    pub const fn bits(self) -> u32 {
        self.0
    }
}

/// All-zero bytes are an empty registry (every tag `FREE`), so the board
/// keeps it NOLOAD and zeroes it at boot.
pub struct StreamRegistry {
    rows: [Row; STREAM_ROWS],
    /// The row the card IRQ holds, index + 1; 0 none.
    held: AtomicU8,
    epoch: AtomicU32,
}

const _: () = assert!(size_of::<StreamRegistry>() <= 2_400);

// SAFETY: a row's file is written only between `insert`'s CAS claim and
// its tag's `Release`, so by one writer, and read only after an `Acquire`
// of that tag; `clear`'s contract keeps a row from being rewritten while a
// reader holds it, and `clear` itself leaves the card's held row.
unsafe impl Sync for StreamRegistry {}

impl Default for StreamRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamRegistry {
    pub const fn new() -> StreamRegistry {
        StreamRegistry {
            rows: [const {
                Row {
                    tag: AtomicU8::new(FREE),
                    damaged: AtomicBool::new(false),
                    stale: AtomicBool::new(false),
                    file: UnsafeCell::new(MaybeUninit::uninit()),
                }
            }; STREAM_ROWS],
            held: AtomicU8::new(0),
            epoch: AtomicU32::new(0),
        }
    }

    /// `f` under `id`, in a free row, if no row holds `id`. The UI's,
    /// before it publishes the source's pool row: rows are inserted from
    /// the UI's thread alone, so the look for `id` holds until the claim.
    pub fn insert(&self, id: SourceId, f: StreamFile) -> Result<(), RowRefused> {
        // A stale row under `id` too: one row an id until its `clear`.
        if self.row_of(id).is_some() {
            return Err(RowRefused::Taken);
        }
        let row = self
            .rows
            .iter()
            .find(|r| {
                r.tag
                    .compare_exchange(FREE, WRITING, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
            })
            .ok_or(RowRefused::Full)?;
        // SAFETY: the CAS made this row ours alone: no reader forms a
        // reference while its tag isn't an id's, and no one held it once
        // freed (`clear`).
        unsafe { (*row.file.get()).write(f) };
        row.damaged.store(false, Ordering::Relaxed);
        row.stale.store(false, Ordering::Relaxed);
        row.tag.store(AtomicTag::encode(id), Ordering::Release);
        Ok(())
    }

    /// Which card the rows' files are on.
    pub fn epoch(&self) -> CardEpoch {
        CardEpoch(self.epoch.load(Ordering::Acquire))
    }

    /// `id`'s file was condemned whole, under any row (a re-head's beside
    /// row streams the same file).
    pub fn damaged(&self, id: SourceId) -> bool {
        let Some(f) = self.get(id).map(StreamFile::file) else {
            return false;
        };
        self.rows.iter().any(|r| {
            let t = r.tag.load(Ordering::Acquire);
            // SAFETY: as `get`: an id's tag, its file written whole.
            t != FREE
                && t != WRITING
                && r.damaged.load(Ordering::Relaxed)
                && unsafe { (*r.file.get()).assume_init_ref() }.file() == f
        })
    }

    /// A row is free: an `insert` of a new id would take it.
    pub fn has_room(&self) -> bool {
        self.rows
            .iter()
            .any(|r| r.tag.load(Ordering::Acquire) == FREE)
    }

    /// The row tagged `id`, stale or not.
    fn row_of(&self, id: SourceId) -> Option<usize> {
        let tag = AtomicTag::encode(id);
        self.rows
            .iter()
            .position(|r| r.tag.load(Ordering::Acquire) == tag)
    }

    /// The live row tagged `id`: none if it is stale.
    fn live_row(&self, id: SourceId) -> Option<usize> {
        self.row_of(id)
            .filter(|&k| !self.rows[k].stale.load(Ordering::Acquire))
    }

    /// `id`'s file, if a live row holds it.
    pub fn get(&self, id: SourceId) -> Option<&StreamFile> {
        let row = &self.rows[self.live_row(id)?];
        // SAFETY: the tag's `Acquire` saw `insert`'s `Release`, after the
        // file was written whole; it stays put while held (`clear`).
        Some(unsafe { (*row.file.get()).assume_init_ref() })
    }

    /// A row streams `file`: the store refuses to delete or replace it
    /// (`FatStore::guard_streamed`).
    pub fn streams(&self, file: FileName) -> bool {
        self.rows.iter().any(|r| {
            let t = r.tag.load(Ordering::Acquire);
            // SAFETY: as `get`: an id's tag, its file written whole.
            t != FREE
                && t != WRITING
                && !r.stale.load(Ordering::Acquire)
                && unsafe { (*r.file.get()).assume_init_ref() }.file() == file
        })
    }

    /// `id`'s file, held for the card IRQ's read: `clear` leaves its row
    /// until the hold drops. One hold at a time: `None` while another
    /// lives, or with no row for `id`.
    pub fn hold(&self, id: SourceId) -> Option<RowHold<'_>> {
        let tag = AtomicTag::encode(id);
        let k = self.live_row(id)?;
        // Under `STREAM_ROWS`.
        let mark = k as u8 + 1;
        self.held
            .compare_exchange(0, mark, Ordering::SeqCst, Ordering::Relaxed)
            .ok()?;
        let hold = RowHold { reg: self, k };
        // The mark, then the tag again; `clear` the tag, then the mark (both
        // `SeqCst`): a `clear` that took the tag first is seen here, and
        // one after sees the mark.
        (self.rows[k].tag.load(Ordering::SeqCst) == tag).then_some(hold)
    }

    /// `id`'s row freed, unless the card holds it (then false: try again
    /// once its read ends). No row for `id` is freed already.
    ///
    /// # Safety
    /// No one holds a reference from `get(id)`, nor will: the source is
    /// retired with its `sounding` bit clear (the retire gate).
    #[must_use]
    pub unsafe fn clear(&self, id: SourceId) -> bool {
        let tag = AtomicTag::encode(id);
        let Some(k) = self.rows.iter().position(|r| {
            r.tag
                .compare_exchange(tag, FREE, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
        }) else {
            return true;
        };
        if self.held.load(Ordering::SeqCst) == k as u8 + 1 {
            // Back before any `insert` (the UI's, as this) can claim it.
            self.rows[k].tag.store(tag, Ordering::Release);
            return false;
        }
        true
    }

    /// Every row freed but the card's held one, which is left stale: true
    /// if none was held.
    ///
    /// # Safety
    /// As `clear`, for every row.
    #[must_use]
    pub unsafe fn clear_all(&self) -> bool {
        // Another card's files from now on.
        self.epoch.fetch_add(1, Ordering::Release);
        // Stale first, so no hold starts between a `clear` that finds a row
        // held and the mark; `insert` makes a row live again.
        for r in &self.rows {
            r.stale.store(true, Ordering::SeqCst);
        }
        // SAFETY: the caller's, for every id.
        // Every id tried, held or not.
        SourceId::ALL
            .into_iter()
            .filter(|&id| !unsafe { self.clear(id) })
            .count()
            == 0
    }
}

/// The card IRQ's hold on a row: its file stays put until this drops.
pub struct RowHold<'r> {
    reg: &'r StreamRegistry,
    k: usize,
}

impl RowHold<'_> {
    /// Its file condemned whole: the SOURCE cell says so.
    pub fn mark_damaged(&self) {
        self.reg.rows[self.k].damaged.store(true, Ordering::Relaxed);
    }

    pub fn file(&self) -> &StreamFile {
        // SAFETY: `hold` saw the row's id tag after marking it, so its file
        // was written whole (`Release`, `Acquire`), and `clear` leaves a
        // marked row: no `insert` rewrites it while this lives.
        unsafe { (*self.reg.rows[self.k].file.get()).assume_init_ref() }
    }
}

impl Drop for RowHold<'_> {
    fn drop(&mut self) {
        self.reg.held.store(0, Ordering::Release);
    }
}
