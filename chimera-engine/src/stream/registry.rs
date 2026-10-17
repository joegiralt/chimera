//! The stream registry (slice 1 § 5.5): a row per streamed source, its
//! `SourceId` and `StreamFile`. The UI writes rows; the card IRQ and the
//! audio read them.
//!
//! Ordering: a row's tag holds its id + 1 (0 free). `insert` claims a free
//! row by CAS, writes its file, then stores the tag `Release`; `get` loads
//! the tag `Acquire` before it reads the file. A row is rewritten only
//! after `clear`, whose caller vouches no reader still holds it.

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicU8, Ordering};

use super::layout::StreamFile;
use crate::project::SourceId;

/// Rows: one a ring, so one a stream.
pub const STREAM_ROWS: usize = 8;

const FREE: u8 = 0;
/// Claimed by `insert`, its file being written.
const WRITING: u8 = u8::MAX;

// Every id + 1 is a tag: neither `FREE` nor `WRITING`.
const _: () = assert!(SourceId::ALL.len() < WRITING as usize);

/// Every row taken: a ninth streamed source refuses to load.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

struct Row {
    tag: AtomicU8,
    file: UnsafeCell<MaybeUninit<StreamFile>>,
}

/// All-zero bytes are an empty registry (every tag `FREE`), so the board
/// keeps it NOLOAD and zeroes it at boot.
pub struct StreamRegistry {
    rows: [Row; STREAM_ROWS],
}

const _: () = assert!(size_of::<StreamRegistry>() <= 2_400);

// SAFETY: a row's file is written only between `insert`'s CAS claim and
// its tag's `Release`, so by one writer, and read only after an `Acquire`
// of that tag; `clear`'s contract keeps a row from being rewritten while a
// reader holds it.
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
                    file: UnsafeCell::new(MaybeUninit::uninit()),
                }
            }; STREAM_ROWS],
        }
    }

    /// `f` under `id`, in a free row. The UI's, before it publishes the
    /// source's pool row.
    pub fn insert(&self, id: SourceId, f: StreamFile) -> Result<(), Full> {
        let row = self
            .rows
            .iter()
            .find(|r| {
                r.tag
                    .compare_exchange(FREE, WRITING, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
            })
            .ok_or(Full)?;
        // SAFETY: the CAS made this row ours alone: no reader forms a
        // reference while its tag isn't an id's, and no one held it once
        // freed (`clear`).
        unsafe { (*row.file.get()).write(f) };
        // Under `WRITING` (asserted above).
        row.tag.store(id.index() as u8 + 1, Ordering::Release);
        Ok(())
    }

    /// `id`'s file, if a row holds it.
    pub fn get(&self, id: SourceId) -> Option<&StreamFile> {
        let tag = id.index() as u8 + 1;
        let row = self
            .rows
            .iter()
            .find(|r| r.tag.load(Ordering::Acquire) == tag)?;
        // SAFETY: the tag's `Acquire` saw `insert`'s `Release`, after the
        // file was written whole; it stays put while held (`clear`).
        Some(unsafe { (*row.file.get()).assume_init_ref() })
    }

    /// `id`'s row freed.
    ///
    /// # Safety
    /// No one holds a reference from `get(id)`, nor will: the source is
    /// retired with its `sounding` bit clear, and the card IRQ has no
    /// read of it in flight (the retire gate).
    pub unsafe fn clear(&self, id: SourceId) {
        let tag = id.index() as u8 + 1;
        for r in &self.rows {
            let _ = r
                .tag
                .compare_exchange(tag, FREE, Ordering::Release, Ordering::Relaxed);
        }
    }

    /// Every row freed.
    ///
    /// # Safety
    /// As `clear`, for every row.
    pub unsafe fn clear_all(&self) {
        for r in &self.rows {
            let _ = r.tag.try_update(Ordering::Release, Ordering::Relaxed, |t| {
                (t != WRITING).then_some(FREE)
            });
        }
    }
}
