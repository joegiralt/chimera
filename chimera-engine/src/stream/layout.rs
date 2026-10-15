//! Streamed chunks in a `.source` file: a chunk is `CHUNK_RECORDS` whole
//! `SourceData` records, read as the raw blocks that hold them and
//! de-framed into packed samples.

use core::ops::Range;

use chimera_hal::store::{Extents, FileRuns, Transfers};
use chimera_hal::units::{BlockCount, Bytes, FileBlock};

use super::units::{Frames, Packing};
use crate::storage::{RECORD_HEAD_LEN, RecordTag, SOURCE_DATA_MAX, SourceLayout, data_offset};

/// A `SourceData` record: tag `u16`, length `u16`, then the samples.
const RECORD_HEAD: usize = RECORD_HEAD_LEN;
const TAG: u16 = RecordTag::SourceData.code();

/// Records in a chunk: the one constant the rest derive from.
pub const CHUNK_RECORDS: u32 = 8;
const RECORD_BYTES: u32 = (RECORD_HEAD + SOURCE_DATA_MAX) as u32;

/// Packed bytes in a full chunk.
pub const CHUNK_BYTES: Bytes = Bytes::new(CHUNK_RECORDS * SOURCE_DATA_MAX as u32);
pub const CHUNK_FRAMES: Frames = Packing::Twelve.frames_in(CHUNK_BYTES);
/// Its records with their heads: what lies in the file.
pub const FRAMED_CHUNK_BYTES: Bytes = Bytes::new(CHUNK_RECORDS * RECORD_BYTES);
/// Blocks a chunk can touch: its bytes starting at the worst skip, 511.
pub const RAW_CHUNK_BLOCKS: u32 = (FRAMED_CHUNK_BYTES.get() + 511).div_ceil(512);
pub const RAW_CHUNK_BYTES: usize = RAW_CHUNK_BLOCKS as usize * 512;

// A chunk starts on a 12-bit pair.
const _: () = assert!(CHUNK_FRAMES.get().is_multiple_of(2));
const _: () = assert!(RAW_CHUNK_BLOCKS * 512 >= FRAMED_CHUNK_BYTES.get() + 511);

const RAW_MAX: BlockCount = match BlockCount::new(RAW_CHUNK_BLOCKS) {
    Some(n) => n,
    None => panic!(),
};

/// A chunk's index: the slot word's 24 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ChunkIdx(u32);

impl ChunkIdx {
    pub const MAX: ChunkIdx = ChunkIdx(0xFF_FFFF);

    pub const fn new(n: u32) -> Option<Self> {
        if n <= Self::MAX.0 {
            Some(Self(n))
        } else {
            None
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    pub const fn next(self) -> Option<Self> {
        Self::new(self.0 + 1)
    }
}

/// The blocks of a chunk, in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkBlocks {
    pub first: FileBlock,
    pub count: BlockCount,
    /// The first record's offset in its first block.
    pub skip: Bytes,
}

/// A source file's samples and where they lie on the card.
#[derive(Clone, Copy, Debug)]
pub struct StreamFile {
    ext: Extents,
    data_off: Bytes,
    data_len: Bytes,
    frames: Frames,
}

impl StreamFile {
    /// `None` when the data runs past `runs.len`, or its chunks past
    /// `ChunkIdx::MAX`.
    pub fn from_layout(l: &SourceLayout, runs: FileRuns) -> Option<StreamFile> {
        let data_len = u32::try_from(l.data_len()).ok()?;
        let records = data_len.div_ceil(SOURCE_DATA_MAX as u32);
        let end = records
            .checked_mul(RECORD_HEAD as u32)?
            .checked_add(data_len)?
            .checked_add(data_offset(l).get())?;
        let sf = StreamFile {
            ext: runs.ext,
            data_off: data_offset(l),
            data_len: Bytes::new(data_len),
            frames: Frames::new(l.head().sref.frames),
        };
        let covered = runs.len.get().div_ceil(512);
        let on_card = covered == 0 || runs.ext.locate(FileBlock::new(covered - 1)).is_some();
        (end <= runs.len.get()
            && on_card
            && data_len > 0
            && ChunkIdx::new(data_len.div_ceil(CHUNK_BYTES.get()) - 1).is_some())
        .then_some(sf)
    }

    pub fn frames(&self) -> Frames {
        self.frames
    }

    pub fn last_chunk(&self) -> ChunkIdx {
        // `from_layout` checked it fits.
        ChunkIdx(self.data_len.get().div_ceil(CHUNK_BYTES.get()) - 1)
    }

    /// The chunk holding `f`; `None` past the end.
    pub fn chunk_of(&self, f: Frames) -> Option<ChunkIdx> {
        (f < self.frames).then(|| ChunkIdx(f.get() / CHUNK_FRAMES.get()))
    }

    /// `None` past `last_chunk`.
    pub fn chunk_frames(&self, c: ChunkIdx) -> Option<Range<Frames>> {
        (c <= self.last_chunk()).then(|| {
            // At most `last_chunk` x 2,688 frames: inside the file's.
            let start = c.0 * CHUNK_FRAMES.get();
            Frames::new(start)..Frames::new((start + CHUNK_FRAMES.get()).min(self.frames.get()))
        })
    }

    /// `None` past `last_chunk`.
    pub fn chunk_blocks(&self, c: ChunkIdx) -> Option<ChunkBlocks> {
        if c > self.last_chunk() {
            return None;
        }
        // Inside the file, whose bytes fit `u32` (`from_layout`).
        let first_p = c.0 * CHUNK_BYTES.get();
        let last_p = (first_p + CHUNK_BYTES.get()).min(self.data_len.get()) - 1;
        let at = |p: u32| {
            self.data_off.get() + RECORD_BYTES * (p / SOURCE_DATA_MAX as u32) + RECORD_HEAD as u32
        };
        // The chunk starts at its first record's head, ends in its last byte.
        let start = at(first_p) - RECORD_HEAD as u32;
        let end = at(last_p) + last_p % SOURCE_DATA_MAX as u32;
        let (first, skip) = Bytes::new(start).split_block();
        let (last, _) = Bytes::new(end).split_block();
        let count = BlockCount::new(last.get() - first.get() + 1)?;
        Some(ChunkBlocks { first, count, skip })
    }

    /// Device reads for `b`: a chunk across a run's edge is two.
    pub fn reads(&self, b: &ChunkBlocks) -> Transfers<'_> {
        self.ext.transfers(b.first, b.count.get(), RAW_MAX)
    }
}

/// Why a chunk's bytes are not samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deframe {
    /// A record that is not `SourceData`.
    Tag,
    /// A record empty or over `SOURCE_DATA_MAX`, or past `out`.
    Length,
    /// `raw` ends before `out` is full.
    Short,
}

/// Copies the samples of the records at `raw[skip..]` into `out`, until
/// it is full; the bytes written.
pub fn deframe(raw: &[u8], skip: Bytes, out: &mut [u8]) -> Result<Bytes, Deframe> {
    let mut at = usize::try_from(skip.get()).map_err(|_| Deframe::Short)?;
    let mut done = 0;
    while done < out.len() {
        let body_at = at.checked_add(RECORD_HEAD).ok_or(Deframe::Short)?;
        let head = raw.get(at..body_at).ok_or(Deframe::Short)?;
        if u16::from_le_bytes([head[0], head[1]]) != TAG {
            return Err(Deframe::Tag);
        }
        let n = usize::from(u16::from_le_bytes([head[2], head[3]]));
        if n == 0 || n > SOURCE_DATA_MAX || n > out.len() - done {
            return Err(Deframe::Length);
        }
        let end = body_at.checked_add(n).ok_or(Deframe::Short)?;
        out[done..done + n].copy_from_slice(raw.get(body_at..end).ok_or(Deframe::Short)?);
        done += n;
        at = end;
    }
    // `out` is a chunk: far under 4 GiB.
    Ok(Bytes::new(u32::try_from(done).unwrap_or(u32::MAX)))
}
