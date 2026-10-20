//! Streamed loads (slice 1 § 5.6): a source too big for the arena loads
//! as its descriptor, head table and heads; the rest stays on the card,
//! its runs in the stream registry for the card interrupt. The SOURCE
//! cell's load and a project load's pass 2 both take this path.

use chimera_hal::store::{FileRuns, Store};
use chimera_hal::units::Bytes;

use super::arena::Cost;
use super::sources::{SOURCE_HEAD, SourceId, SourceSet, source_cost};
use super::{PoolFull, Project};
use crate::dsp::tape_engine::params::{NeedsRam, TapeEngineParams};
use crate::dsp::tape_engine::read::Shape;
use crate::storage::{LoadError, Ready, SourceHead, SourceLayout, SourceReadError, SourceRef};
use crate::stream::layout::StreamFile;
use crate::stream::stage::{Heads, heads_for};

mod load;

/// How a card file loads for a Part (spec § 5.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadKind {
    /// It fits the arena whole.
    Whole,
    /// Too big: its heads, the rest streamed.
    Streamed(Heads),
    /// Too big, and it can't stream: why.
    Refused(Refusal),
}

/// Why a card file too big for the arena won't load (spec § 5.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The Part plays it a way that needs it whole.
    NeedsRam(NeedsRam),
    /// A transwave or zone set: only a plain source streams.
    Shape(Shape),
    /// Too few frames past START for a head.
    TooShort,
    /// Bank 2 shut: no heads are placed (TAPE is silent then).
    Closed,
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
/// if it fits, else streamed if `p` plays it forward (a `Plain` only),
/// else refused, saying why.
pub fn load_kind(h: &SourceHead, free: Cost, p: &TapeEngineParams) -> LoadKind {
    let whole = SourceLayout::whole(*h)
        .map(|l| l.held_len())
        .or_else(|| crate::storage::held_len(h.sref));
    if whole.is_some_and(|n| source_cost(n) <= free) {
        return LoadKind::Whole;
    }
    match (h.shape, heads_for(h.sref, p)) {
        (Shape::Plain, Ok(heads)) => LoadKind::Streamed(heads),
        (_, Err(why @ Refusal::NeedsRam(_))) => LoadKind::Refused(why),
        (Shape::Transwave | Shape::ZoneSet, _) => LoadKind::Refused(Refusal::Shape(h.shape)),
        (Shape::Plain, Err(why)) => LoadKind::Refused(why),
    }
}

/// What bank 1 may call of a streamed load (ADR 0137): the heads'
/// placement, the load's passes and a project load's streamed sources,
/// all bank 2's with `bank2` (`load`, `stage::place`).
#[repr(C)]
pub struct LoadDoor {
    heads_for: fn(SourceRef, &TapeEngineParams) -> Result<Heads, Refusal>,
    step: fn(&mut StreamLoad, &mut dyn Store, &Ready, &mut Project, usize) -> StreamStep,
    sources: fn(&mut dyn Store, &Ready, &mut Project, SourceSet) -> u8,
}

impl LoadDoor {
    pub const DOOR: LoadDoor = LoadDoor {
        heads_for: crate::stream::stage::heads_in,
        step: load::step,
        sources: load::stream_sources,
    };

    #[inline(always)]
    pub(crate) fn heads_for(&self, r: SourceRef, p: &TapeEngineParams) -> Result<Heads, Refusal> {
        (self.heads_for)(r, p)
    }

    #[inline(always)]
    pub(crate) fn step(
        &self,
        l: &mut StreamLoad,
        s: &mut dyn Store,
        r: &Ready,
        p: &mut Project,
        budget: usize,
    ) -> StreamStep {
        (self.step)(l, s, r, p, budget)
    }

    #[cfg(feature = "tape")]
    #[inline(always)]
    pub(crate) fn sources(
        &self,
        s: &mut dyn Store,
        r: &Ready,
        p: &mut Project,
        ids: SourceSet,
    ) -> u8 {
        (self.sources)(s, r, p, ids)
    }

    /// `f` on the door: bank 2's through the door with `bank2`, `None`
    /// closed; this build's own without.
    #[inline(always)]
    pub(crate) fn with<R>(f: impl FnOnce(&LoadDoor) -> R) -> Option<R> {
        #[cfg(feature = "bank2")]
        return crate::bank2::DOOR_CELL.with(|d| f(&d.load));
        #[cfg(not(feature = "bank2"))]
        Some(f(&Self::DOOR))
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
    /// A re-head's source sounds and its heads couldn't go in beside it
    /// (nothing free, or the pool refused): it waits (#588, #598).
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
    phase: Phase,
    /// The arena offset the staging started at: a gap that moved fails it.
    stage: u32,
}

/// Where a `StreamLoad` is.
#[derive(Clone, Copy)]
enum Phase {
    /// The first pass finds the file's runs.
    Find,
    /// The heads read: `at` bytes so far, of every head's.
    Heads { file: StreamFile, at: Bytes },
}

impl Phase {
    fn heads(file: StreamFile) -> Phase {
        Phase::Heads {
            file,
            at: Bytes::new(0),
        }
    }
}

impl StreamLoad {
    /// Card file `head` as `heads`; `None` when they don't fit the gap.
    pub fn start(p: &mut Project, head: SourceHead, heads: Heads) -> Option<StreamLoad> {
        let (stage, _) = p.source_staging(heads.held_len())?;
        Some(StreamLoad {
            sref: head.sref,
            heads,
            rehead: None,
            phase: Phase::Find,
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
            phase: Phase::heads(file),
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
            phase: Phase::heads(file),
            stage,
        })
    }

    /// The source it loads.
    pub fn sref(&self) -> SourceRef {
        self.sref
    }

    /// The first pass finds the file's runs and checks its head; each
    /// after reads up to `budget` bytes of heads; the last writes the
    /// descriptor and table, the registry row, then the pool row. With
    /// bank 2 shut it fails as a missing file.
    pub fn step(
        &mut self,
        s: &mut dyn Store,
        r: &Ready,
        p: &mut Project,
        budget: usize,
    ) -> StreamStep {
        LoadDoor::with(|d| d.step(self, s, r, p, budget)).unwrap_or(StreamStep::Failed(
            StreamFail::Read(SourceReadError::Load(LoadError::Missing)),
        ))
    }
}
