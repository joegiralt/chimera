//! Streamed loads (slice 1 § 5.6): a source too big for the arena loads
//! as its descriptor, head table and heads; the rest stays on the card,
//! its runs in the stream registry for the card interrupt. The SOURCE
//! cell's load and a project load's pass 2 both take this path.

use chimera_hal::store::{FileRuns, RunsError, Store, StoreError};
use chimera_hal::units::Bytes;

use super::arena::Cost;
use super::pool::StreamRefused;
use super::sources::{SOURCE_HEAD, SourceId, source_cost};
use super::{PoolFull, Project};
use crate::dsp::tape_engine::params::{NeedsRam, TapeEngineParams, needs_ram};
use crate::dsp::tape_engine::read::{Descriptor, Shape, write_head_table};
use crate::storage::{
    LoadError, Ready, SourceHead, SourceLayout, SourceReadError, SourceRef, read_packed_into,
    read_source_head, source_file,
};
use crate::stream::admit::head_frames;
use crate::stream::layout::StreamFile;
use crate::stream::units::{Frames, Packing};
use crate::stream::stage::{Heads, heads_for};

/// How a card file loads for a Part (spec § 5.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadKind {
    /// It fits the arena whole.
    Whole,
    /// Too big: its heads, the rest streamed.
    Streamed(Heads),
    /// Too big, and the Part plays it in a way that needs it whole.
    Refused(NeedsRam),
}

impl LoadKind {
    /// What it takes of the arena: `whole` held, or the heads' row.
    pub fn cost(&self, whole: Cost) -> Cost {
        match self {
            LoadKind::Streamed(h) => Cost::of(SOURCE_HEAD + h.held_len()),
            _ => whole,
        }
    }
}

/// Card file `h` for a Part playing it with `p`, `free` bytes left: whole
/// if it fits, else streamed if `p` plays it forward (a `Plain` only).
pub fn load_kind(h: &SourceHead, free: Cost, p: &TapeEngineParams) -> LoadKind {
    let whole = SourceLayout::whole(*h)
        .map(|l| l.held_len())
        .or_else(|| crate::storage::held_len(h.sref));
    if whole.is_some_and(|n| source_cost(n) <= free) {
        return LoadKind::Whole;
    }
    match (h.shape, heads_for(h.sref, p)) {
        (Shape::Plain, Some(heads)) => LoadKind::Streamed(heads),
        _ => LoadKind::Refused(needs_ram(p).unwrap_or(NeedsRam::Machine(p.model))),
    }
}

/// Why a streamed load published nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamFail {
    /// In more runs than the registry keeps: `FRAGMENTED · COPY IT AGAIN`.
    Fragmented,
    /// Every stream row taken: `STREAMS FULL`.
    Full,
    /// The heads no longer fit the gap, or every pool slot is taken.
    Pool(PoolFull),
    /// Missing, damaged framing, or a card fault.
    Read(SourceReadError),
    /// A re-head's source sounds: it waits for its notes (#588).
    Sounding,
}

/// One pass of a `StreamLoad`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamStep {
    More,
    Done(SourceId),
    Failed(StreamFail),
}

/// A streamed load: the heads staged in the gap's bottom, a UI pass's
/// share at a time, then the registry row, then the pool row. Nothing
/// moves or publishes before, so dropping it is a cancel.
pub struct StreamLoad {
    sref: SourceRef,
    heads: Heads,
    /// A pool row's heads re-staged, not a new row published.
    rehead: Option<SourceId>,
    /// The file's runs, once found: the first pass's.
    file: Option<StreamFile>,
    /// Head bytes read so far, of every head's.
    at: usize,
    /// The arena offset the staging started at: a gap that moved fails it.
    stage: u32,
}

impl StreamLoad {
    /// Card file `head` as `heads`; `None` when they don't fit the gap.
    pub fn start(p: &mut Project, head: SourceHead, heads: Heads) -> Option<StreamLoad> {
        let (stage, _) = p.source_staging(heads.held_len())?;
        Some(StreamLoad {
            sref: head.sref,
            heads,
            rehead: None,
            file: None,
            at: 0,
            stage,
        })
    }

    /// `start`, its file's runs found and its head read already (a
    /// project load's pass 2): no first pass. `None` as `start`, or when
    /// the runs don't hold its samples.
    pub fn found(
        p: &mut Project,
        head: SourceHead,
        heads: Heads,
        runs: FileRuns,
    ) -> Option<StreamLoad> {
        let file = StreamFile::from_layout(&SourceLayout::whole(head)?, runs)?;
        let l = Self::start(p, head, heads)?;
        Some(StreamLoad {
            file: Some(file),
            ..l
        })
    }

    /// Streamed pool source `id`'s heads re-staged as `heads` (spec § 5.6:
    /// START or the loop turned); `None` when it isn't streamed, or they
    /// don't fit the gap beside its old ones.
    pub fn rehead(p: &mut Project, id: SourceId, heads: Heads) -> Option<StreamLoad> {
        let file = *p.pool.streams()?.get(id)?;
        let sref = p.pool.source(id)?.r;
        let (stage, _) = p.source_staging(heads.held_len())?;
        Some(StreamLoad {
            sref,
            heads,
            rehead: Some(id),
            file: Some(file),
            at: 0,
            stage,
        })
    }

    /// The source it loads.
    pub fn sref(&self) -> SourceRef {
        self.sref
    }

    /// The first pass finds the file's runs and checks its head; each
    /// after reads up to `budget` bytes of heads; the last writes the
    /// descriptor and table, the registry row, then the pool row.
    pub fn step<S: Store>(
        &mut self,
        s: &mut S,
        r: &Ready,
        p: &mut Project,
        budget: usize,
    ) -> StreamStep {
        match self.pass(s, r, p, budget) {
            Ok(Some(id)) => StreamStep::Done(id),
            Ok(None) => StreamStep::More,
            Err(f) => StreamStep::Failed(f),
        }
    }

    fn pass<S: Store>(
        &mut self,
        s: &mut S,
        r: &Ready,
        p: &mut Project,
        budget: usize,
    ) -> Result<Option<SourceId>, StreamFail> {
        let read = |e: StoreError| StreamFail::Read(e.into());
        let Some(file) = self.file else {
            let name = source_file(self.sref);
            let runs = s.runs(r.volume(), name).map_err(|e| match e {
                RunsError::Fragmented(_) => StreamFail::Fragmented,
                RunsError::Store(StoreError::NotFound) => {
                    StreamFail::Read(SourceReadError::Load(LoadError::Missing))
                }
                RunsError::Store(e) => read(e),
            })?;
            let got =
                read_source_head(s, r, self.sref.crc, runs.len.get()).map_err(StreamFail::Read)?;
            let bad = StreamFail::Read(SourceReadError::Load(LoadError::File(
                crate::storage::FileError::Corrupt,
            )));
            let l = SourceLayout::whole(got).filter(|_| got.sref == self.sref);
            self.file = Some(
                l.and_then(|l| StreamFile::from_layout(&l, runs))
                    .ok_or(bad)?,
            );
            return Ok(None);
        };
        let len = self.heads.held_len();
        let gone = StreamFail::Pool(PoolFull {
            needs: source_cost(len),
            free: p.pool().free(),
        });
        let (stage, held) = p.source_staging(len).ok_or(gone)?;
        if stage != self.stage {
            return Err(gone);
        }
        let d = Descriptor {
            shape: Shape::Plain,
            count: 1,
            rate: self.sref.rate,
            head_frames: head_frames(self.sref.rate),
            heads: 0,
        };
        let table = write_head_table(d, self.heads.as_slice(), held).ok_or(gone)?;
        // The head the next bytes are in, and how far into it.
        let mut before = 0;
        for h in self.heads.as_slice() {
            if self.at < before + h.byte_len() {
                let into = self.at - before;
                let n = (h.byte_len() - into).min(budget.max(3) / 3 * 3);
                let out = &mut held[table + self.at..][..n];
                let from = Frames::new(h.first())
                    .to_bytes(Packing::Twelve)
                    .checked_add(Bytes::new(into as u32))
                    .ok_or(gone)?;
                read_packed_into(s, r, self.sref, from, out).map_err(StreamFail::Read)?;
                self.at += n;
                return Ok(None);
            }
            before += h.byte_len();
        }
        if let Some(id) = self.rehead {
            p.bump();
            return match p.pool.source_rehead_timed(id, len) {
                true => Ok(Some(id)),
                false => Err(StreamFail::Sounding),
            };
        }
        p.bump();
        p.pool
            .source_commit_streamed(self.sref, len, file)
            .map(Some)
            .map_err(|e| match e {
                StreamRefused::Pool(e) => StreamFail::Pool(e),
                StreamRefused::Full => StreamFail::Full,
                StreamRefused::Taken => StreamFail::Read(SourceReadError::Load(LoadError::File(
                    crate::storage::FileError::Corrupt,
                ))),
            })
    }
}
