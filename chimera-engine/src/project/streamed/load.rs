// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! A streamed load's work: the passes that read a source's heads and
//! publish its rows, and a project load's streamed sources. With `bank2`
//! it is bank 2's (`bank2.x` globs this module); bank 1 reaches it only
//! through `LoadDoor`.

use chimera_hal::store::{RunsError, Seal, Store, StoreError};
use chimera_hal::units::Bytes;

use super::super::arena::{ARENA_BYTES, Cost};
use super::super::pool::StreamRefused;
use super::super::sources::source_cost;
use super::super::{Absence, PartId, PoolFull, Project, SourceId, SourceSet};
use super::{
    LoadKind, NotFound, Phase, StreamFail, StreamLoad, StreamStep, StreamedPass, load_kind,
};
use crate::dsp::tape_engine::read::{DESC_LEN, write_head_table};
use crate::storage::{
    CardError, CardFault, Crc32, LoadError, Ready, SourceLayout, SourceReadError, held_len,
    read_packed_into, read_source_head, source_file,
};
use crate::stream::admit::head_frames;
use crate::stream::layout::{FileKey, StreamFile};
use crate::stream::units::{Frames, Packing, SourceRate};

/// A head past any file's bytes, or a table no descriptor holds: cold, so
/// the pass after it isn't.
#[cold]
#[optimize(size)]
fn corrupted() -> StreamFail {
    StreamFail::Read(SourceReadError::Load(LoadError::File(
        crate::storage::FileError::Corrupt,
    )))
}

/// The card itself failed: not a busy bus (the card untouched), nor a
/// file missing or damaged (`CardError::from_store`). Only that ends a
/// project load's stream pass (#761 m9).
#[optimize(size)]
fn card_faulted(e: &impl CardFault) -> bool {
    e.store_error().and_then(CardError::from_store).is_some()
}

/// One pass of `l` (`StreamLoad::step`).
#[optimize(size)]
pub(super) fn step(
    l: &mut StreamLoad,
    s: &mut dyn Store,
    r: &Ready,
    p: &mut Project,
    budget: usize,
) -> StreamStep {
    match l.pass(s, r, p, budget) {
        Ok(Some(id)) => StreamStep::Done(id),
        Ok(None) => StreamStep::More,
        Err(f) => StreamStep::Failed(f),
    }
}

impl StreamLoad {
    #[optimize(size)]
    fn pass(
        &mut self,
        s: &mut dyn Store,
        r: &Ready,
        p: &mut Project,
        budget: usize,
    ) -> Result<Option<SourceId>, StreamFail> {
        // Every pass on the card the first one read (#1083).
        if *self.on.get_or_insert(r.volume()) != r.volume() {
            return Err(StreamFail::Read(SourceReadError::Load(LoadError::Missing)));
        }
        let (file, at) = match self.phase {
            Phase::Heads { file, at } => (file, at),
            Phase::Find => return self.find(s, r).map(|()| None),
            Phase::Check { file, crc, at } => {
                return self.check(s, r, p, budget, file, crc, at).map(|()| None);
            }
        };
        let at = at.get() as usize;
        let len = self.heads.held_len();
        let gone = StreamFail::Pool(PoolFull {
            needs: source_cost(len),
            free: p.pool().free(),
        });
        let (stage, held) = p.source_staging(len).ok_or(gone)?;
        // The gap moved since the last pass (a source in or out below it):
        // nothing is published yet, so its heads are staged again there.
        if stage != self.stage {
            self.stage = stage;
            self.phase = Phase::heads(file);
            return Ok(None);
        }
        // A head past any file's bytes, or a table longer than its row:
        // the head's numbers are wrong, not the pool's room.
        let table = SourceRate::new(self.sref.rate)
            .and_then(|r| {
                write_head_table(r, Frames::new(head_frames(r)), self.heads.as_slice(), held)
            })
            .ok_or_else(corrupted)?;
        // The head the next bytes are in, and how far into it.
        let mut before = 0;
        for h in self.heads.as_slice() {
            if at < before + h.byte_len() {
                let into = at - before;
                let n = (h.byte_len() - into).min(budget.max(3) / 3 * 3);
                let out = &mut held[table + at..][..n];
                let from = Frames::new(h.first())
                    .to_bytes(Packing::Twelve)
                    .checked_add(Bytes::new(into as u32))
                    .ok_or_else(corrupted)?;
                read_packed_into(s, r, self.sref, from, out).map_err(StreamFail::Read)?;
                let at = Bytes::new((at + n) as u32);
                self.phase = Phase::Heads { file, at };
                return Ok(None);
            }
            before += h.byte_len();
        }
        self.still_found(s, r, p, &file)?;
        p.bump();
        if let Some(id) = self.rehead {
            return p
                .pool
                .source_rehead_timed(id, len)
                .map(Some)
                .ok_or(StreamFail::Sounding);
        }
        // Rows of another card than this read's go first (#1083).
        follow(p, r.volume());
        p.pool
            .source_commit_streamed(self.sref, len, file, r.volume())
            .map(Some)
            .map_err(|e| match e {
                StreamRefused::Pool(e) => StreamFail::Pool(e),
                StreamRefused::Full => StreamFail::Full,
                // The registry and the pool disagree on a row: unreachable,
                // as a drain keeps its id `Draining`, and a failed publish
                // keeps it owed (`free_source`), until `clear` succeeds.
                // If it ever fires it is no card's fault: STREAMS FULL.
                StreamRefused::Taken => StreamFail::Full,
            })
    }

    /// The first pass: the file's runs and its head; an unsealed file is
    /// checked before its heads (`check`).
    #[optimize(size)]
    fn find(&mut self, s: &mut dyn Store, r: &Ready) -> Result<(), StreamFail> {
        let runs = s
            .runs(r.volume(), source_file(self.sref))
            .map_err(|e| match e {
                RunsError::Fragmented(_) => StreamFail::Fragmented,
                RunsError::Store(StoreError::NotFound) => {
                    StreamFail::Read(SourceReadError::Load(LoadError::Missing))
                }
                RunsError::Store(e) => StreamFail::Read(e.into()),
            })?;
        let got =
            read_source_head(s, r, self.sref.stem(), runs.len.get()).map_err(StreamFail::Read)?;
        let l = SourceLayout::whole(got).filter(|_| got.sref == self.sref);
        let file = l.and_then(|l| StreamFile::from_layout(&l, runs));
        let file = file.ok_or_else(corrupted)?;
        // Unsealed, a source never read back whole against its CRC
        // (copied onto the card by hand, the host dropping the read-only
        // bit, or a cut `put`'s remains): streaming reads only its heads,
        // so its samples are hashed first (#761 m10, ADR 0210). The CRC
        // starts as `source_crc`'s, on the head's shape and count.
        self.phase = if runs.seal == Seal::Sealed {
            Phase::heads(file)
        } else {
            let mut crc = Crc32::new();
            crc.update(&[got.shape as u8, got.count]);
            Phase::Check { file, crc, at: 0 }
        };
        Ok(())
    }

    /// An unsealed file's next share of packed bytes into the staging,
    /// hashed (ADR 0210): a pass's budget, at most the staging's room.
    /// Once they all hash to its name, on the card and the file as found,
    /// it is sealed and its heads come next; one that doesn't stays
    /// unsealed and refused. The seal is best effort: a card that won't
    /// take it streams the checked file anyway, checked again next load.
    #[expect(clippy::too_many_arguments, reason = "the phase's fields, unpacked")]
    #[optimize(size)]
    fn check(
        &mut self,
        s: &mut dyn Store,
        r: &Ready,
        p: &mut Project,
        budget: usize,
        file: StreamFile,
        mut crc: Crc32,
        at: u32,
    ) -> Result<(), StreamFail> {
        let total = held_len(self.sref)
            .and_then(|n| u32::try_from(n.checked_sub(DESC_LEN)?).ok())
            .ok_or_else(corrupted)?;
        let len = self.heads.held_len();
        let gone = StreamFail::Pool(PoolFull {
            needs: source_cost(len),
            free: p.pool().free(),
        });
        let (_, held) = p.source_staging(len).ok_or(gone)?;
        let n = ((total - at) as usize).min(budget.max(3)).min(held.len());
        let out = &mut held[..n];
        read_packed_into(s, r, self.sref, Bytes::new(at), out).map_err(StreamFail::Read)?;
        crc.update(out);
        let at = at + n as u32;
        if at < total {
            self.phase = Phase::Check { file, crc, at };
            return Ok(());
        }
        if crc.finish() != self.sref.crc {
            return Err(corrupted());
        }
        self.still_found(s, r, p, &file)?;
        let _ = s.seal(r.volume(), source_file(self.sref));
        self.phase = Phase::heads(file);
        Ok(())
    }

    /// The file as found, on the card as found: a remount onto another
    /// volume, or the file deleted or replaced since, publishes nothing
    /// (#761 m3).
    #[optimize(size)]
    fn still_found(
        &self,
        s: &mut dyn Store,
        r: &Ready,
        p: &Project,
        file: &StreamFile,
    ) -> Result<(), StreamFail> {
        let missing = StreamFail::Read(SourceReadError::Load(LoadError::Missing));
        if p.pool.streams().map(|g| g.epoch()) != self.epoch {
            return Err(missing);
        }
        if self.rehead.is_some() {
            return Ok(());
        }
        let now = match s.runs(r.volume(), source_file(self.sref)) {
            Ok(n) => FileKey::new(self.sref.stem(), n.ext.first()),
            Err(RunsError::Store(e)) if e != StoreError::NotFound => {
                return Err(StreamFail::Read(e.into()));
            }
            Err(_) => return Err(missing),
        };
        if now == file.key() {
            Ok(())
        } else {
            Err(missing)
        }
    }
}

/// The sources `ids`, too big for the arena, that a TAPE Part plays
/// forward load as their heads (slice 1 § 5.6), as the SOURCE cell's load
/// does; one every stream row refuses, in too many runs, or with no room
/// for its heads stays absent and says why. Returns how many streamed,
/// and those that fit whole since another's read missed, for the loader
/// to read whole (#765). A card fault ends them.
// Not size-optimised: its frame grows on the stack check's deepest chain (ADR 0267).
pub(super) fn stream_sources(
    s: &mut dyn Store,
    r: &Ready,
    p: &mut Project,
    ids: SourceSet,
) -> StreamedPass {
    let (mut n, mut fault, mut whole) = (0u8, false, SourceSet::EMPTY);
    for id in ids.iter() {
        let Some(e) = p.pool.source(id) else { continue };
        // A card fault ends the reads: the rest read as failed.
        if fault {
            p.pool.mark_absent(id, Absence::ReadFailed);
            continue;
        }
        let tape = PartId::ALL
            .into_iter()
            .map(|q| &p.part(q).sound)
            .find(|x| x.engine() == crate::params::EngineType::Tape && x.source_ref() == Some(e.r))
            .map(|x| x.params.tape);
        let Some(tape) = tape else { continue };
        let runs = match s.runs(r.volume(), crate::storage::source_file(e.r)) {
            Ok(runs) => runs,
            Err(RunsError::Fragmented(_)) => {
                p.pool.mark_absent(id, Absence::Fragmented);
                continue;
            }
            Err(RunsError::Store(err)) => {
                if card_faulted(&SourceReadError::from(err)) {
                    p.pool.mark_absent(id, Absence::ReadFailed);
                    fault = true;
                }
                continue;
            }
        };
        let head = match read_source_head(s, r, e.r.stem(), runs.len.get()) {
            // The stem alone matched: another source's file under it is
            // not this one (as `StreamLoad::pass` and `read_sources` judge).
            Ok(head) if head.sref != e.r => {
                p.pool.mark_absent(id, Absence::Missing);
                continue;
            }
            Ok(head) => head,
            Err(err) => {
                p.pool.mark_absent(id, Absence::ReadFailed);
                fault = card_faulted(&err);
                continue;
            }
        };
        let heads = match load_kind(&head, p.pool.free(), &tape) {
            LoadKind::Streamed(heads) => heads,
            LoadKind::Refused(why) => {
                p.pool.mark_absent(id, Absence::NeedsRam(why));
                continue;
            }
            // Room since: read whole after this pass (#765).
            LoadKind::Whole => {
                whole = whole.with(id);
                continue;
            }
        };
        let mut l = match StreamLoad::found(p, head, heads, runs) {
            Ok(l) => l,
            Err(NotFound::File) => {
                p.pool.mark_absent(id, Absence::Missing);
                continue;
            }
            Err(NotFound::NoRoom) => {
                p.pool.mark_absent(id, Absence::NoRoom);
                continue;
            }
        };
        // One card operation, as `read_sources`' whole reads (ADR 0087),
        // `ARENA_BYTES` a pass: the heads are the arena's, so they take one
        // or two; an unsealed source's check first reads its samples
        // whole, once, a staging's room a pass (ADR 0210).
        let why = loop {
            match step(&mut l, s, r, p, ARENA_BYTES) {
                StreamStep::More => {}
                StreamStep::Done(_) => {
                    n = n.saturating_add(1);
                    break None;
                }
                StreamStep::Failed(StreamFail::Full) => break Some(Absence::StreamsFull),
                StreamStep::Failed(StreamFail::Fragmented) => break Some(Absence::Fragmented),
                StreamStep::Failed(StreamFail::Read(err)) => {
                    fault = card_faulted(&err);
                    break Some(Absence::ReadFailed);
                }
                // The heads didn't fit; `Sounding` is a re-head's alone.
                StreamStep::Failed(StreamFail::Pool(_) | StreamFail::Sounding) => {
                    break Some(Absence::NoRoom);
                }
            }
        };
        if let Some(w) = why {
            p.pool.mark_absent(id, w);
        }
    }
    StreamedPass { n, whole }
}

/// `Project::follow_card`, bank 2's (#1083). Out of line: its
/// `lose_streams` stays off the streamed load's frame (the stack check's
/// deepest chain runs through it).
#[inline(never)]
pub(super) fn follow(p: &mut Project, v: chimera_hal::store::VolumeId) -> bool {
    p.follow_card_in(v)
}

/// `LoadDoor`'s `ask` entry.
pub(super) fn ask(a: super::LoadAsk<'_>) {
    match a {
        super::LoadAsk::Sources { s, r, p, ids, out } => *out = stream_sources(s, r, p, ids),
        super::LoadAsk::Follow { p, v, out } => *out = follow(p, v),
        super::LoadAsk::PartHeads { stage, refs, heads } => part_heads(stage, refs, heads),
        super::LoadAsk::PlanHeads {
            part_used,
            taken,
            heads,
            left,
        } => {
            for (k, h) in heads.iter().enumerate() {
                let bit = 1 << k;
                let c = h.map(|h| h.cost().bytes());
                if part_used & bit != 0
                    && taken & bit == 0
                    && let Some(c) = c.filter(|&c| c <= *left)
                {
                    *left -= c;
                }
            }
        }
    }
}

/// `LoadAsk::PartHeads` (#600).
fn part_heads(
    stage: &crate::preset::Sound,
    refs: &[crate::project::SourceRef],
    heads: &mut [Option<super::HeadsCost>],
) {
    let Some(r) = stage
        .source_ref()
        .filter(|_| stage.engine() == crate::params::EngineType::Tape)
    else {
        return;
    };
    let Some(k) = refs.iter().position(|&x| x == r) else {
        return;
    };
    if heads[k].is_none() {
        heads[k] = crate::stream::stage::heads_in(r, &stage.params.tape)
            .ok()
            .and_then(|h| {
                super::HeadsCost::of(Cost::of(super::super::sources::SOURCE_HEAD + h.held_len()))
            });
    }
}
