//! Streamed chunks in a `.source` file: a chunk is `CHUNK_RECORDS` whole
//! `SourceData` records, read as the raw blocks that hold them and
//! de-framed into packed samples.

use core::ops::Range;

use chimera_hal::store::{Extents, FileName, FileRuns, Transfers};
use chimera_hal::units::{BlockCount, Bytes, FileBlock, Lba};

use super::units::{Frames, Packing, SourceFrame};
use crate::storage::{
    RECORD_HEAD_LEN, RecordTag, SOURCE_DATA_MAX, SourceLayout, data_offset, source_name,
};

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

    /// The chunk holding frame `f`: every frame's fits.
    pub const fn of(f: SourceFrame) -> Self {
        Self(f.get() / CHUNK_FRAMES.get())
    }

    /// `n` chunks on, at most `MAX`.
    pub const fn saturating_add(self, n: u32) -> Self {
        let c = self.0.saturating_add(n);
        Self(if c < Self::MAX.0 { c } else { Self::MAX.0 })
    }

    /// Its first frame; `SourceFrame::MAX` past `u32`.
    pub const fn first_frame(self) -> SourceFrame {
        SourceFrame::new(self.0.saturating_mul(CHUNK_FRAMES.get()))
    }

    /// The chunks `self..=to`, in order.
    pub fn through(self, to: ChunkIdx) -> impl Iterator<Item = ChunkIdx> {
        (self.0..=to.0).map(ChunkIdx)
    }
}

// `ChunkIdx::of` holds every `u32` frame.
const _: () = assert!(u32::MAX / CHUNK_FRAMES.get() <= ChunkIdx::MAX.0);

/// The blocks of a chunk, in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkBlocks {
    pub first: FileBlock,
    pub count: BlockCount,
    /// The first record's offset in its first block.
    pub skip: Bytes,
}

/// One `.SRC` where it lies: its stem (its content's CRC) and its first
/// block (#609). A copy of the same content elsewhere, or on another card,
/// is another key; a source id isn't one, as ids are reused once drained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileKey {
    stem: u32,
    at: Lba,
}

impl FileKey {
    pub const fn new(stem: u32, at: Lba) -> FileKey {
        FileKey { stem, at }
    }
}

/// A chunk of one file by its first raw block: the same bytes at the same
/// place, what a frame error blames (#609).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileChunk {
    file: FileKey,
    at: Lba,
}

impl FileChunk {
    pub const fn new(file: FileKey, at: Lba) -> FileChunk {
        FileChunk { file, at }
    }

    pub const fn file(self) -> FileKey {
        self.file
    }
}

/// A source file's samples and where they lie on the card.
#[derive(Clone, Copy, Debug)]
pub struct StreamFile {
    ext: Extents,
    /// Its `.SRC`'s stem: the file a delete must not free under a stream.
    stem: u32,
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
            stem: l.head().sref.crc,
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

    /// The `.SRC` it streams from (`source_name`).
    pub fn file(&self) -> FileName {
        source_name(self.stem)
    }

    pub fn frames(&self) -> Frames {
        self.frames
    }

    /// The file where it lies, as a frame error names it.
    pub fn key(&self) -> FileKey {
        FileKey::new(self.stem, self.ext.first())
    }

    pub fn last_chunk(&self) -> ChunkIdx {
        // `from_layout` checked it fits.
        ChunkIdx(self.data_len.get().div_ceil(CHUNK_BYTES.get()) - 1)
    }

    /// The chunk holding `f`; `None` past the end.
    pub fn chunk_of(&self, f: SourceFrame) -> Option<ChunkIdx> {
        (f.get() < self.frames.get()).then(|| ChunkIdx::of(f))
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

    /// Chunk `c`'s reads, a CMD18 a piece; `None` past `last_chunk`, or
    /// where its pieces don't cover its blocks.
    pub fn chunk_read(&self, c: ChunkIdx) -> Option<ChunkRead<'_>> {
        let blocks = self.chunk_blocks(c)?;
        let covered: u32 = self.reads(&blocks).map(|(_, n)| n.get()).sum();
        let len =
            Bytes::new((self.data_len.get() - c.0 * CHUNK_BYTES.get()).min(CHUNK_BYTES.get()));
        piece(&self.ext, &blocks, 0)?;
        (covered == blocks.count.get() && blocks.count.get() <= RAW_CHUNK_BLOCKS)
            .then_some(self.read_of(ChunkPlan { blocks, len }))
    }

    /// The read `chunk_read` found, from its plan: no second look.
    pub(crate) fn read_of(&self, p: ChunkPlan) -> ChunkRead<'_> {
        ChunkRead {
            file: self,
            blocks: p.blocks,
            len: p.len,
        }
    }
}

/// A `ChunkRead` less its file, kept beside the held row
/// (`serve::HeldRead`) so a fill works its read out once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ChunkPlan {
    blocks: ChunkBlocks,
    len: Bytes,
}

/// One CMD18 of a chunk: its device blocks, landing `at` blocks into the
/// staging, after the pieces before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub lba: Lba,
    pub blocks: BlockCount,
    pub at: u32,
}

/// A chunk's reads: pieces that cover its blocks in order, at most a raw
/// chunk's in all, so they land contiguously in one staging.
#[derive(Clone, Copy, Debug)]
pub struct ChunkRead<'f> {
    file: &'f StreamFile,
    blocks: ChunkBlocks,
    len: Bytes,
}

impl ChunkRead<'_> {
    pub(crate) fn plan(&self) -> ChunkPlan {
        ChunkPlan {
            blocks: self.blocks,
            len: self.len,
        }
    }

    /// Every piece's blocks.
    pub fn blocks(&self) -> BlockCount {
        self.blocks.count
    }

    /// Where the first record starts in the first block.
    pub fn skip(&self) -> Bytes {
        self.blocks.skip
    }

    /// The chunk's packed bytes.
    pub fn packed(&self) -> Bytes {
        self.len
    }

    /// The chunk where it lies, as a frame error names it; always `Some`,
    /// as `first`.
    pub fn key(&self) -> Option<FileChunk> {
        Some(FileChunk::new(self.file.key(), self.first()?.lba))
    }

    /// Always `Some` (`chunk_read` found it); an `Option` so the read
    /// holds no copy of it.
    pub fn first(&self) -> Option<Piece> {
        piece(&self.file.ext, &self.blocks, 0)
    }

    /// The piece after `p`; `None` after the last.
    pub fn after(&self, p: &Piece) -> Option<Piece> {
        piece(
            &self.file.ext,
            &self.blocks,
            p.at.checked_add(p.blocks.get())?,
        )
    }
}

/// The piece of `b` that starts `at` blocks in.
fn piece(ext: &Extents, b: &ChunkBlocks, at: u32) -> Option<Piece> {
    let from = FileBlock::new(b.first.get().checked_add(at)?);
    let left = b.count.get().checked_sub(at)?;
    let (lba, blocks) = ext.transfers(from, left, RAW_MAX).next()?;
    Some(Piece { lba, blocks, at })
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
