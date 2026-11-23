// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Streamed loads (slice 1 § 5.6): a source too big for the arena loads
//! as its descriptor, head table and heads; the rest stays on the card,
//! its runs in the stream registry for the card interrupt. The SOURCE
//! cell's load and a project load's pass 2 both take this path.

use chimera_hal::store::{FileRuns, Seal, Store, VolumeId};
use chimera_hal::units::Bytes;

use super::arena::Cost;
use super::sources::{SOURCE_HEAD, SourceId, SourceSet, source_cost};
use super::{PoolFull, Project};
use crate::dsp::tape_engine::params::{NeedsRam, TapeEngineParams};
use crate::dsp::tape_engine::read::Shape;
use crate::storage::{
    Crc32, LoadError, Ready, SourceHead, SourceLayout, SourceReadError, SourceRef,
};
use crate::stream::layout::StreamFile;
use crate::stream::registry::CardEpoch;
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
    /// A transwave: only a plain source streams.
    Transwave,
    /// A zone set: only a plain source streams.
    ZoneSet,
    /// Too few frames past START for a head.
    TooShort,
    /// Bank 2 shut: no heads are placed (TAPE is silent then).
    Closed,
}

impl LoadKind {
    /// What it takes of the arena: `whole` held, or the heads' row.
    #[optimize(size)]
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
// Not size-optimised: its frame grows on the stack check's deepest chain (ADR 0267).
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
        (Shape::Transwave, _) => LoadKind::Refused(Refusal::Transwave),
        (Shape::ZoneSet, _) => LoadKind::Refused(Refusal::ZoneSet),
        (Shape::Plain, Err(why)) => LoadKind::Refused(why),
    }
}

/// A project load's streamed pass: how many it streamed, and the sources
/// left out for room that now fit whole, another's read having missed:
/// the loader reads those whole after it (#765).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamedPass {
    pub n: u8,
    pub whole: SourceSet,
}

/// What bank 1 may call of a streamed load (ADR 0137): the heads'
/// placement, the load's passes and a project load's streamed sources,
/// all bank 2's with `bank2` (`load`, `stage::place`).
#[repr(C)]
pub struct LoadDoor {
    heads_for: fn(SourceRef, &TapeEngineParams) -> Result<Heads, Refusal>,
    step: fn(&mut StreamLoad, &mut dyn Store, &Ready, &mut Project, usize) -> StreamStep,
    /// The rest, one entry: the door table is full (`LoadAsk`).
    ask: fn(LoadAsk<'_>),
}

/// What a source's heads cost the arena, in granules (#600): never 0,
/// and any that fits the arena fits the word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadsCost(core::num::NonZeroU16);

const _: () = assert!(super::arena::ARENA_BYTES / super::arena::GRANULE <= u16::MAX as usize);

impl HeadsCost {
    /// `c`, if it fits the arena at all.
    pub fn of(c: super::arena::Cost) -> Option<HeadsCost> {
        let g = c.bytes() as usize / super::arena::GRANULE;
        (c.bytes() as usize <= super::arena::ARENA_BYTES)
            .then(|| core::num::NonZeroU16::new(g as u16))
            .flatten()
            .map(HeadsCost)
    }

    pub fn cost(self) -> super::arena::Cost {
        super::arena::Cost::of(usize::from(self.0.get()) * super::arena::GRANULE)
    }
}

/// `LoadDoor`'s other questions, one entry for all.
pub enum LoadAsk<'a> {
    /// A project load's streamed pass (`load::stream_sources`).
    Sources {
        s: &'a mut dyn Store,
        r: &'a Ready,
        p: &'a mut Project,
        ids: SourceSet,
        out: &'a mut StreamedPass,
    },
    /// The stream rows follow card `v` (#1083: `Project::follow_card`).
    Follow {
        p: &'a mut Project,
        v: VolumeId,
        out: &'a mut bool,
    },
    /// #600: pass 1's Part Sound `stage`: if TAPE and the first on its
    /// source (one of `refs`), its heads' cost into `heads`, bytes (0:
    /// it can't stream).
    PartHeads {
        stage: &'a crate::preset::Sound,
        refs: &'a [SourceRef],
        heads: &'a mut [Option<HeadsCost>],
    },
    /// #600: the plan takes the heads of each Part source (`part_used`)
    /// not taken whole or missing (`taken`) out of `left`.
    PlanHeads {
        part_used: u32,
        taken: u32,
        heads: &'a [Option<HeadsCost>],
        left: &'a mut u32,
    },
}

impl LoadDoor {
    pub const DOOR: LoadDoor = LoadDoor {
        heads_for: crate::stream::stage::heads_in,
        step: load::step,
        ask: load::ask,
    };

    #[inline(always)]
    #[optimize(size)]
    pub(crate) fn heads_for(&self, r: SourceRef, p: &TapeEngineParams) -> Result<Heads, Refusal> {
        (self.heads_for)(r, p)
    }

    #[inline(always)]
    #[optimize(size)]
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
    #[optimize(size)]
    pub(crate) fn sources(
        &self,
        s: &mut dyn Store,
        r: &Ready,
        p: &mut Project,
        ids: SourceSet,
    ) -> StreamedPass {
        let mut out = StreamedPass::default();
        (self.ask)(LoadAsk::Sources {
            s,
            r,
            p,
            ids,
            out: &mut out,
        });
        out
    }

    #[inline(always)]
    #[optimize(size)]
    pub(crate) fn follow(&self, p: &mut Project, v: VolumeId) -> bool {
        let mut out = false;
        (self.ask)(LoadAsk::Follow {
            p,
            v,
            out: &mut out,
        });
        out
    }

    #[inline(always)]
    #[optimize(size)]
    pub(crate) fn ask(&self, a: LoadAsk<'_>) {
        (self.ask)(a)
    }

    /// `f` on the door: bank 2's through the door with `bank2`, `None`
    /// closed; this build's own without.
    #[inline(always)]
    #[optimize(size)]
    pub(crate) fn with<R>(f: impl FnOnce(&LoadDoor) -> R) -> Option<R> {
        #[cfg(feature = "bank2")]
        return crate::bank2::DOOR_CELL.with(|d| f(&d.load));
        #[cfg(not(feature = "bank2"))]
        Some(f(&Self::DOOR))
    }
}

/// Why `StreamLoad::found` started nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotFound {
    /// The file's runs don't hold its samples: `SOURCE MISSING`.
    File,
    /// Its heads don't fit the gap: `POOL FULL` (#765).
    NoRoom,
}

/// Why a streamed load published nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamFail {
    /// In more runs than the registry keeps: `FRAGMENTED · SILENT`.
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
    /// The registry's card when it started: another one since (a re-init
    /// that mounted another volume, `lose_streams`) refuses the commit, so
    /// no row names the old card's blocks.
    epoch: Option<CardEpoch>,
    /// The volume its first pass read: a pass on another refuses (#1083),
    /// whether or not any row followed the card.
    on: Option<VolumeId>,
}

/// Where a `StreamLoad` is.
#[derive(Clone, Copy)]
enum Phase {
    /// The first pass finds the file's runs.
    Find,
    /// An unsealed file's packed bytes hashed against its name, a pass's
    /// share at a time through the staging: `at` bytes so far (ADR 0210).
    Check {
        file: StreamFile,
        crc: Crc32,
        at: u32,
    },
    /// The heads read: `at` bytes so far, of every head's.
    Heads { file: StreamFile, at: Bytes },
}

impl Phase {
    #[optimize(size)]
    fn heads(file: StreamFile) -> Phase {
        Phase::Heads {
            file,
            at: Bytes::new(0),
        }
    }
}

impl StreamLoad {
    /// Card file `head` as `heads`; `None` when they don't fit the gap.
    #[optimize(size)]
    pub fn start(p: &mut Project, head: SourceHead, heads: Heads) -> Option<StreamLoad> {
        let (stage, _) = p.source_staging(heads.held_len())?;
        Some(StreamLoad {
            sref: head.sref,
            heads,
            rehead: None,
            phase: Phase::Find,
            stage,
            epoch: p.pool.streams().map(|r| r.epoch()),
            on: None,
        })
    }

    /// `start`, its file's runs found and its head read already (a
    /// project load's pass 2): no first pass, unless the file is unsealed
    /// (never checked against its CRC): that first pass checks it whole
    /// and seals it. Refused, saying why, as `start` (no room for the
    /// heads) or when the runs don't hold its samples.
    #[optimize(size)]
    pub fn found(
        p: &mut Project,
        head: SourceHead,
        heads: Heads,
        runs: FileRuns,
    ) -> Result<StreamLoad, NotFound> {
        let file = SourceLayout::whole(head)
            .and_then(|l| StreamFile::from_layout(&l, runs))
            .ok_or(NotFound::File)?;
        let l = Self::start(p, head, heads).ok_or(NotFound::NoRoom)?;
        if runs.seal != Seal::Sealed {
            return Ok(l);
        }
        Ok(StreamLoad {
            phase: Phase::heads(file),
            ..l
        })
    }

    /// Streamed pool source `id`'s heads re-staged as `heads` (spec § 5.6:
    /// START or the loop turned); `None` when it isn't streamed, or they
    /// don't fit the gap beside its old ones.
    #[optimize(size)]
    pub fn rehead(p: &mut Project, id: SourceId, heads: Heads) -> Option<StreamLoad> {
        let reg = p.pool.streams()?;
        let (file, epoch) = (*reg.get(id)?, reg.epoch());
        let sref = p.pool.source(id)?.r;
        let (stage, _) = p.source_staging(heads.held_len())?;
        Some(StreamLoad {
            sref,
            heads,
            rehead: Some(id),
            phase: Phase::heads(file),
            stage,
            epoch: Some(epoch),
            on: p.pool.streams_on,
        })
    }

    /// The source it loads.
    #[optimize(size)]
    pub fn sref(&self) -> SourceRef {
        self.sref
    }

    /// The first pass finds the file's runs and checks its head; each
    /// after reads up to `budget` bytes of heads; the last writes the
    /// descriptor and table, the registry row, then the pool row. With
    /// bank 2 shut it fails as a missing file.
    #[optimize(size)]
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
