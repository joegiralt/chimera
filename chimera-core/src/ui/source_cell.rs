//! TAPE's SOURCE cell (spec § 3): turned in place through the pool's
//! sources, a divider, then the card's `/CHIMERA/TAPES` files the pool
//! doesn't hold, each with its cost. MIX + PLUS loads a card row, 4 KiB a
//! UI pass behind BUSY; MENU cancels. The Digitakt's SAMPLE SLOT, with the
//! Octatrack's audio pool behind it.
//!
//! Per-source memory is CHOMPI's per-slot memory (ideas only; ADR 0093):
//! RAM only, keyed by the source's CRC.

use core::ops::ControlFlow;

use core::num::NonZeroU32;

use chimera_hal::card_bus::CardFault;
use chimera_hal::store::{ReadSink, Store, StoreError};

use crate::dsp::tape_engine::params::TapeEngineParams;
#[cfg(feature = "tape")]
use crate::dsp::tape_engine::read::head_table;
use crate::dsp::tape_engine::{NeedsRam, TapeModel};
use crate::preset::Sound;
use crate::project::arena::{Cost, MAX_SOURCES};
use crate::project::{
    Absence, PartId, Pool, Project, SOURCE_HEAD, SourceId, SourceNeed, StreamFail,
};
pub use crate::project::{LoadKind, load_kind};
#[cfg(feature = "tape")]
use crate::project::{StreamLoad, StreamStep};
use crate::storage::{
    Card, CardEvent, MAX_TAPES, Ready, SourceCheck, SourceHead, SourceRef, TapeList, TapeSource,
    after_mount, held_for_file_len, source_file,
};
#[cfg(feature = "tape")]
use crate::stream::stage::{Heads, heads_for};
use crate::ui::view::SourceState;

/// What one UI pass reads of a card-row load, and of a head.
pub const STEP_BYTES: u32 = 4096;
/// Heads kept for their names (spec § 3).
pub const HEAD_CACHE: usize = 16;

pub const SOURCE_MISSING: &str = "SOURCE MISSING";
pub const NOT_ON_CARD: &str = "SOURCE NOT ON CARD";
pub const READ_FAILED: &str = "CARD READ FAILED";
/// A source in more runs than the stream registry keeps (slice 1 § 5.3).
pub const FRAGMENTED: &str = "FRAGMENTED · COPY IT AGAIN";
/// A streamed file the card condemned whole (#609): records that won't
/// de-frame, so its voices drop out over them. Its cell reads `BAD FILE`:
/// `DAMAGED` is a pixel past the column, `FILE DAMAGED · COPY AGAIN` past
/// the band.
pub const FILE_DAMAGED: &str = "BAD FILE · COPY AGAIN";
/// A ninth streamed source: the registry's eight rows taken.
pub const STREAMS_FULL: &str = "STREAMS FULL";
/// A streamed source reads forward only until slice 5 (slice 1 § 2.3).
pub const PING_NEEDS_RAM: &str = "PING NEEDS RAM";
pub const REVERSE_NEEDS_RAM: &str = "REVERSE NEEDS RAM";

/// Why a streamed source won't play: the focus band's words.
pub fn needs_ram_words(n: NeedsRam) -> &'static str {
    match n {
        NeedsRam::Ping => PING_NEEDS_RAM,
        NeedsRam::Reverse => REVERSE_NEEDS_RAM,
        // The machines that seek (slice 1 § 2.7): provisional copy.
        NeedsRam::Machine(TapeModel::Scrub) => "SCRUB NEEDS RAM",
        NeedsRam::Machine(TapeModel::Stretch) => "STRETCH NEEDS RAM",
        NeedsRam::Machine(TapeModel::Transwave) => "WAVE NEEDS RAM",
        NeedsRam::Machine(TapeModel::Tron) => "TRON NEEDS RAM",
        NeedsRam::Machine(TapeModel::Play) => "NEEDS RAM",
    }
}

/// Why a streamed load published nothing: the focus band's words.
pub fn stream_fail_words(f: StreamFail) -> &'static str {
    match f {
        StreamFail::Fragmented => FRAGMENTED,
        StreamFail::Full => STREAMS_FULL,
        StreamFail::Pool(_) => crate::ui::settings::prompt::POOL_FULL,
        StreamFail::Read(crate::storage::SourceReadError::Load(
            crate::storage::LoadError::Missing,
        )) => NOT_ON_CARD,
        StreamFail::Read(_) | StreamFail::Sounding => READ_FAILED,
    }
}

/// A step of SOURCE's turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRow {
    Pool(SourceId),
    /// Between the pool's and the card's: never stopped on.
    Divider,
    /// A card file the pool doesn't hold, by its stem (the CRC).
    Card {
        stem: u32,
        cost: Cost,
    },
}

/// A card file's cost from its listed size, with no read; `None` for a
/// size no whole file has.
pub fn card_cost(size: u32) -> Option<Cost> {
    held_for_file_len(size).map(|h| Cost::of(SOURCE_HEAD + h as usize))
}

fn held(pool: &Pool, s: SourceId) -> Option<SourceRef> {
    pool.source(s).filter(|e| !e.absent()).map(|e| e.r)
}

/// The pool's sources, a divider, then `card`'s files (stem, size) the
/// pool doesn't hold. No divider with no card row.
pub fn rows<'a>(pool: &'a Pool, card: &'a [(u32, u32)]) -> impl Iterator<Item = SourceRow> + 'a {
    let in_pool = move |stem| {
        SourceId::ALL
            .into_iter()
            .any(|s| held(pool, s).is_some_and(|r| r.crc == stem))
    };
    let cards = card.iter().filter_map(move |&(stem, size)| {
        let cost = card_cost(size).filter(|_| !in_pool(stem))?;
        Some(SourceRow::Card { stem, cost })
    });
    let divider = cards.clone().next().map(|_| SourceRow::Divider);
    SourceId::ALL
        .into_iter()
        .filter(move |&s| held(pool, s).is_some())
        .map(SourceRow::Pool)
        .chain(divider)
        .chain(cards)
}

/// The rows SOURCE stops on, in order: `rows` less the divider.
#[inline(never)]
fn stops(pool: &Pool, card: &[(u32, u32)]) -> ([SourceRow; MAX_SOURCES + MAX_TAPES], usize) {
    let mut out = [SourceRow::Divider; MAX_SOURCES + MAX_TAPES];
    let mut n = 0;
    for r in rows(pool, card).filter(|r| *r != SourceRow::Divider) {
        out[n] = r;
        n += 1;
    }
    (out, n)
}

/// What a card read's failure reads: a file gone, or anything else.
pub fn said(e: StoreError) -> &'static str {
    match e {
        StoreError::NotFound => NOT_ON_CARD,
        _ => READ_FAILED,
    }
}

fn file(stem: u32) -> chimera_hal::store::FileName {
    source_file(SourceRef {
        crc: stem,
        frames: 0,
        rate: 0,
    })
}

/// Feeds a read to a `SourceCheck` until `left` bytes are taken.
struct Feed<'a> {
    check: &'a mut SourceCheck,
    held: Option<&'a mut [u8]>,
    left: u32,
    got: u32,
    refused: bool,
}

impl ReadSink for Feed<'_> {
    fn begin(&mut self, _len: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        let n = bytes.len().min(self.left as usize);
        if self
            .check
            .feed_into(&bytes[..n], self.held.as_deref_mut())
            .is_err()
        {
            self.refused = true;
            return ControlFlow::Break(());
        }
        self.left -= n as u32;
        self.got += n as u32;
        if self.left == 0 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// A card file's head, from its first 4 KB: a card row's name, rate and
/// length, and a pick's ROOT and PERIOD.
pub fn read_head<S: Store>(
    s: &mut S,
    r: &Ready,
    stem: u32,
    size: u32,
) -> Result<SourceHead, &'static str> {
    let mut check = SourceCheck::new(stem, size).map_err(|_| READ_FAILED)?;
    let mut feed = Feed {
        check: &mut check,
        held: None,
        left: STEP_BYTES,
        got: 0,
        refused: false,
    };
    s.read_at(r.volume(), file(stem), 0, &mut feed)
        .map_err(said)?;
    check.head().ok_or(READ_FAILED)
}

/// A card row loading: `held` staged in the gap's bottom through
/// `SourceCheck`, which checks the CRC and writes the descriptor; only a
/// whole, passing file publishes. Nothing moves or publishes before, so
/// dropping it mid-load is a cancel: the gap and the meter are as they were.
pub struct SourceLoad {
    stem: u32,
    size: u32,
    /// Bytes read so far.
    at: u32,
    /// What the file holds, from its size (`held_for_file_len`).
    held: usize,
    /// The arena offset the staging started at: a gap that moved under it
    /// fails the load.
    stage: u32,
    check: SourceCheck,
}

/// One pass of a `SourceLoad`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadStep {
    More,
    Done(SourceId, SourceHead),
    Failed(&'static str),
}

/// One pass of a `SourceLoad` that doesn't publish.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StagedStep {
    More,
    /// Whole and passing in the staging, unpublished.
    Staged(SourceNeed),
    Failed(&'static str),
}

impl SourceLoad {
    /// Card file `stem` of `size` bytes, into `p`'s gap; `None` when it
    /// won't fit.
    pub fn start(p: &mut Project, stem: u32, size: u32) -> Option<SourceLoad> {
        let held = held_for_file_len(size)? as usize;
        let (stage, _) = p.source_staging(held)?;
        Some(SourceLoad {
            stem,
            size,
            at: 0,
            held,
            stage,
            check: SourceCheck::new(stem, size).ok()?,
        })
    }

    /// Reads the next 4 KiB into the staging; once the file is whole and
    /// passes, publishes it with `Release`.
    pub fn step<S: Store>(&mut self, s: &mut S, r: &Ready, p: &mut Project) -> LoadStep {
        match self.step_staged(s, r, p) {
            StagedStep::More => LoadStep::More,
            StagedStep::Failed(w) => LoadStep::Failed(w),
            StagedStep::Staged(need) => {
                match (p.source_commit(need.r, need.held), self.check.head()) {
                    (Ok(id), Some(head)) => LoadStep::Done(id, head),
                    _ => LoadStep::Failed(crate::ui::settings::prompt::POOL_FULL),
                }
            }
        }
    }

    /// `step` without the commit: the last chunk leaves the whole, passing
    /// file staged, unpublished, and says what it is. A library load's
    /// `adopt` publishes it.
    pub fn step_staged<S: Store>(&mut self, s: &mut S, r: &Ready, p: &mut Project) -> StagedStep {
        // A gap that moved under the staging: never published.
        let Some((stage, held)) = p.source_staging(self.held) else {
            return StagedStep::Failed(READ_FAILED);
        };
        if stage != self.stage {
            return StagedStep::Failed(READ_FAILED);
        }
        let mut feed = Feed {
            check: &mut self.check,
            held: Some(held),
            left: STEP_BYTES,
            got: 0,
            refused: false,
        };
        let read = s.read_at(r.volume(), file(self.stem), self.at, &mut feed);
        let (got, refused) = (feed.got, feed.refused);
        if let Err(e) = read {
            return StagedStep::Failed(said(e));
        }
        if refused || got == 0 {
            return StagedStep::Failed(READ_FAILED);
        }
        self.at += got;
        if self.at < self.size {
            return StagedStep::More;
        }
        // Every byte came and passed: the last feed checked the CRC.
        match (self.check.head(), self.check.held_len()) {
            (Some(head), Some(held)) => StagedStep::Staged(SourceNeed {
                r: head.sref,
                held,
                on_card: true,
            }),
            _ => StagedStep::Failed(READ_FAILED),
        }
    }
}

/// The heads read so far, oldest first; full, the oldest goes.
#[derive(Clone, Copy, Debug)]
pub struct HeadCache([Option<SourceHead>; HEAD_CACHE]);

impl HeadCache {
    pub const EMPTY: HeadCache = HeadCache([None; HEAD_CACHE]);

    pub fn get(&self, crc: u32) -> Option<SourceHead> {
        self.0.iter().flatten().find(|h| h.sref.crc == crc).copied()
    }

    pub fn put(&mut self, h: SourceHead) {
        push(&mut self.0, |x| x.sref.crc == h.sref.crc, h);
    }
}

/// `v` into `slots` kept oldest first: in place of the one `same` finds,
/// else after the last, else over the oldest.
fn push<T: Copy, const N: usize>(slots: &mut [Option<T>; N], same: impl Fn(&T) -> bool, v: T) {
    let n = slots.iter().position(Option::is_none).unwrap_or(N);
    let at = slots[..n].iter().flatten().position(same);
    let from = at.unwrap_or(if n == N { 0 } else { n });
    if from < n {
        slots.copy_within(from + 1..n, from);
    }
    slots[n.min(N) - usize::from(from < n)] = Some(v);
}

/// A Part's START, END, PITCH and FINE on one source. A CRC of 0 is
/// never remembered: it gives `Option<Memo>` its niche, 16 B.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Memo {
    pub crc: NonZeroU32,
    pub start: f32,
    pub end: f32,
    pub pitch: i8,
    pub fine: i8,
}

const _: () = assert!(size_of::<Option<Memo>>() == 16);

/// Per-source memory (ADR 0093): turning SOURCE away from a source keeps
/// the Part's START, END, PITCH and FINE on it; turning back restores
/// them. RAM only (owner, 2026-10-05): a saved Sound keeps its own
/// source's values, and nothing new reaches the card. Oldest first; full,
/// the oldest goes.
#[derive(Clone, Copy, Debug)]
pub struct SourceMemo([Option<Memo>; MAX_SOURCES]);

impl SourceMemo {
    pub const EMPTY: SourceMemo = SourceMemo([None; MAX_SOURCES]);

    pub fn get(&self, crc: u32) -> Option<Memo> {
        self.0
            .iter()
            .flatten()
            .find(|m| m.crc.get() == crc)
            .copied()
    }

    /// `sound`'s values kept for its source, if it has one.
    pub fn keep(&mut self, sound: &Sound) {
        let Some(crc) = sound.source_ref().and_then(|r| NonZeroU32::new(r.crc)) else {
            return;
        };
        let (t, p) = (&sound.params.tape, &sound.params.pitch);
        let m = Memo {
            crc,
            start: t.start,
            end: t.end,
            pitch: p.pitch as i8,
            fine: p.fine as i8,
        };
        push(&mut self.0, |x| x.crc == crc, m);
    }

    /// SOURCE turned on `sound` to `to`: its values on the old source
    /// kept, then `to`'s restored; an unseen source takes START 0 and END 1
    /// and keeps PITCH and FINE. START latches at note-on and END at a
    /// wrap, so nothing sounding snaps. A known head's ROOT and PERIOD come
    /// with it (spec § 1).
    pub fn turn(&mut self, sound: &mut Sound, to: SourceRef, head: Option<&SourceHead>) {
        self.keep(sound);
        match head {
            Some(h) if h.sref == to => sound.pick_source(h, None),
            _ => sound.source = Some(TapeSource::from(to)),
        }
        let (t, p) = (&mut sound.params.tape, &mut sound.params.pitch);
        match self.get(to.crc) {
            Some(m) => {
                (t.start, t.end) = (m.start, m.end);
                (p.pitch, p.fine) = (f32::from(m.pitch), f32::from(m.fine));
            }
            None => (t.start, t.end) = (0.0, 1.0),
        }
    }
}

/// Card work SOURCE queued: run by `card_work`, one a UI pass, under BUSY.
enum SourceJob {
    /// List the card, then turn.
    List(PartId, i8),
    /// Read a head for its name; with `pick`, its ROOT and PERIOD go to
    /// `part`'s Sound if it still plays that source.
    Head {
        part: PartId,
        stem: u32,
        size: u32,
        pick: bool,
    },
    Load(PartId, SourceLoad),
    /// A card row too big for the arena, loading as its heads.
    #[cfg(feature = "tape")]
    Stream(PartId, StreamLoad),
    /// A streamed source's heads re-staging: no BUSY, no words.
    #[cfg(feature = "tape")]
    Rehead(StreamLoad),
    /// A library load's source, held for `UiState` to step: never run
    /// here. It shares this slot, as the two never stream at once.
    Stage(SourceLoad),
}

/// The SOURCE cell's state beside the project: the card as listed, the
/// heads read, the card row it's on, its queued card work, a failure to
/// show until the next input, and the per-source memory.
pub struct SourceCell {
    pub memo: SourceMemo,
    heads: HeadCache,
    /// `None`: listed again at the next turn.
    card: Option<TapeList>,
    /// The card row the cell is on, on that Part: (stem, size).
    at: Option<(PartId, u32, u32)>,
    job: Option<SourceJob>,
    note: Option<&'static str>,
    /// A streamed source whose heads its Part moved off, and since how
    /// many frames.
    #[cfg(feature = "tape")]
    wait: Option<Wait>,
    /// Each source's heads whose re-stage failed for a reason that stays
    /// (`heads_key`; 0 none): not tried again until it wants others.
    #[cfg(feature = "tape")]
    stuck: [u32; MAX_SOURCES],
    /// The card IRQ's read fault, as the last UI pass met it.
    read: Recovery,
}

/// A stream read fault, from the UI's side: the card IRQ sets it, a mount
/// clears it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Recovery {
    Clear,
    /// A re-init and mount owed: this pass, and the next while the bus is
    /// busy (the card untouched, C1).
    Owed,
    /// The re-init failed: left until a card op's mount clears it, so the
    /// UI never stalls on a card that won't come back.
    Stuck,
}

/// Heads wanted for a streamed source, unchanged for `frames` frames.
#[cfg(feature = "tape")]
#[derive(Clone, Copy)]
struct Wait {
    id: SourceId,
    heads: Heads,
    frames: u16,
}

/// How long the heads wait after the last turn (spec § 5.6).
pub const REHEAD: chimera_hal::units::Millis = chimera_hal::units::Millis::new(200);

/// `heads`, folded to a word that is never 0 (FNV-1a).
#[cfg(feature = "tape")]
fn heads_key(heads: &Heads) -> u32 {
    let words = heads.as_slice().iter().flat_map(|h| [h.first(), h.end()]);
    words.fold(0x811C_9DC5, |k, w| (k ^ w).wrapping_mul(0x0100_0193)) | 1
}

/// The first streamed source a Part's START, loop or XFADE moved off its
/// heads that `ready` lets re-stage, and the heads it wants. A source two
/// Parts play takes its first Part's heads (#599).
#[cfg(feature = "tape")]
fn moved_heads(p: &Project, ready: impl Fn(SourceId, &Heads) -> bool) -> Option<(SourceId, Heads)> {
    let pool = p.pool();
    let mut seen = 0u32;
    PartId::ALL.into_iter().find_map(|part| {
        let sound = &p.part(part).sound;
        let r = sound
            .source_ref()
            .filter(|_| sound.engine() == crate::params::EngineType::Tape)?;
        let id = pool.find_source(r)?;
        let first = seen & id.bit() == 0;
        seen |= id.bit();
        let staged = head_table(pool.held(r)?)?;
        let want = heads_for(r, &sound.params.tape)?;
        let moved =
            first && !staged.is_empty() && !staged.iter().eq(want.as_slice().iter().copied());
        (moved && ready(id, &want)).then_some((id, want))
    })
}

impl SourceCell {
    pub const NEW: SourceCell = SourceCell {
        memo: SourceMemo::EMPTY,
        heads: HeadCache::EMPTY,
        card: None,
        at: None,
        job: None,
        note: None,
        #[cfg(feature = "tape")]
        wait: None,
        #[cfg(feature = "tape")]
        stuck: [0; MAX_SOURCES],
        read: Recovery::Clear,
    };

    /// Once a pass, before any card work: under a stream read fault the
    /// card re-inited and mounted (`Store::recover`). Another card's volume
    /// takes the streamed sources with it (`Project::lose_streams`).
    pub fn recover<S: Store>(&mut self, card: &mut Card, s: &mut S, p: &mut Project) {
        self.read = match (s.read_fault(), self.read) {
            (CardFault::Clear, _) => Recovery::Clear,
            (CardFault::Read, Recovery::Stuck) => Recovery::Stuck,
            (CardFault::Read, _) => match s.recover() {
                Err(StoreError::Busy) => Recovery::Owed,
                r => {
                    let (c, event) = after_mount(*card, r);
                    *card = c;
                    if event.is_some_and(|e| e != CardEvent::Same) {
                        p.lose_streams();
                        #[cfg(feature = "tape")]
                        {
                            self.stuck = [0; MAX_SOURCES];
                        }
                    }
                    if r.is_ok() {
                        Recovery::Clear
                    } else {
                        Recovery::Stuck
                    }
                }
            },
        };
    }

    /// Card work is queued: BUSY shows. A re-head runs unseen.
    pub fn busy(&self) -> bool {
        #[cfg(feature = "tape")]
        if matches!(self.job, Some(SourceJob::Rehead(_))) {
            return false;
        }
        self.job.is_some()
    }

    /// Staging in the gap: the pool's holes wait (`UiState::update`).
    pub fn holds_gap(&self) -> bool {
        #[cfg(feature = "tape")]
        if matches!(self.job, Some(SourceJob::Rehead(_))) {
            return true;
        }
        self.loading()
    }

    /// Once a frame: a streamed source whose Part moved START, the loop or
    /// XFADE off its heads re-stages them `REHEAD` after the last turn;
    /// a note holding it keeps the old ones (spec § 5.6, #588, #598).
    /// Beside no other card work.
    #[cfg_attr(not(feature = "tape"), allow(unused_variables))]
    pub fn tick(&mut self, p: &mut Project) {
        #[cfg(feature = "tape")]
        self.rehead_tick(p);
    }

    #[cfg(feature = "tape")]
    fn rehead_tick(&mut self, p: &mut Project) {
        if self.job.is_some() {
            return;
        }
        // Neither a source that can't re-stage yet (a note holds it, and
        // nothing is free beside it: #598) nor one stuck on these heads
        // keeps the others waiting.
        let pool = p.pool();
        let ready =
            |id: SourceId, h: &Heads| pool.can_rehead(id) && self.stuck[id.index()] != heads_key(h);
        let Some((id, heads)) = moved_heads(p, ready) else {
            self.wait = None;
            return;
        };
        let w = match self.wait {
            Some(w) if w.id == id && w.heads == heads => w,
            _ => Wait {
                id,
                heads,
                frames: 0,
            },
        };
        let frames = w.frames.saturating_add(1);
        self.wait = Some(Wait { frames, ..w });
        if u32::from(frames) * crate::ui::animation::FRAME_MS >= REHEAD.get() {
            self.job = StreamLoad::rehead(p, id, heads).map(SourceJob::Rehead);
        }
    }

    pub fn loading(&self) -> bool {
        #[cfg(feature = "tape")]
        if matches!(self.job, Some(SourceJob::Stream(..))) {
            return true;
        }
        matches!(self.job, Some(SourceJob::Load(..)))
    }

    /// MENU during a load: dropped, so nothing was published and the gap
    /// is as it was.
    pub fn cancel(&mut self) {
        if self.loading() {
            self.job = None;
        }
    }

    /// A library load's source into the job slot: BUSY, and no SOURCE
    /// card work starts beside it.
    pub(crate) fn hold(&mut self, l: SourceLoad) {
        self.job = Some(SourceJob::Stage(l));
    }

    /// The library load's source, if held.
    pub(crate) fn staging(&mut self) -> Option<&mut SourceLoad> {
        match &mut self.job {
            Some(SourceJob::Stage(l)) => Some(l),
            _ => None,
        }
    }

    /// The library load's source dropped: nothing was published.
    pub(crate) fn unhold(&mut self) {
        if matches!(self.job, Some(SourceJob::Stage(_))) {
            self.job = None;
        }
    }

    /// Any input retires a failure's words.
    pub fn input(&mut self) {
        self.note = None;
    }

    /// A page entered: off any card row; the card is listed again.
    pub fn leave(&mut self) {
        (self.at, self.card) = (None, None);
    }

    /// A project loaded: the memory starts empty (RAM only).
    pub fn forget(&mut self) {
        *self = SourceCell::NEW;
    }

    /// SOURCE on `part` as the cell reads it: a failure, the card row
    /// it's on, or `base` named from the head cache.
    pub fn state(&self, part: PartId, base: SourceState, p: &Project) -> SourceState {
        if let Some(w) = self.note {
            return SourceState::Failed(w);
        }
        if let Some((_, stem, size)) = self.at.filter(|a| a.0 == part) {
            let whole = card_cost(size).unwrap_or_default();
            return SourceState::Card {
                stem,
                head: self
                    .heads
                    .get(stem)
                    .map(|h| (h.name, h.sref.rate, h.sref.frames)),
                cost: self
                    .kind(p, part, stem)
                    .map_or(whole, |(_, k)| k.cost(whole)),
                free: p.pool().free(),
            };
        }
        let r = p.part(part).sound.source_ref();
        if self.read != Recovery::Clear && r.is_some_and(|r| p.pool().streamed(r)) {
            return SourceState::Absent(Absence::ReadFailed);
        }
        base.named(r.and_then(|r| self.heads.get(r.crc)).map(|h| h.name))
    }

    fn size(&self, stem: u32) -> Option<u32> {
        let files = self.card.as_ref()?.files();
        files.iter().find(|f| f.0 == stem).map(|f| f.1)
    }

    /// SOURCE turned `delta` detents on `part`: a pool row plays at once;
    /// a card row is only shown, dimmed, until MIX + PLUS loads it.
    pub fn turn(&mut self, p: &mut Project, part: PartId, delta: i8) {
        let Some(card) = self.card else {
            self.job = Some(SourceJob::List(part, delta));
            return;
        };
        let playing = p.part(part).sound.source_ref();
        let on = |row: &SourceRow| match (*row, self.at) {
            (SourceRow::Card { stem, .. }, Some((q, at, _))) => q == part && stem == at,
            (SourceRow::Pool(s), _) => {
                self.at.is_none_or(|a| a.0 != part) && held(p.pool(), s) == playing
            }
            _ => false,
        };
        let (steps, n) = stops(p.pool(), card.files());
        let steps = &steps[..n];
        let n = n as i32;
        let from = steps
            .iter()
            .position(on)
            .map_or(if delta > 0 { -1 } else { n }, |i| i as i32);
        let to = (from + i32::from(delta)).min(n - 1).max(0);
        let Some(&row) = steps.get(to as usize) else {
            return;
        };
        match row {
            SourceRow::Pool(s) => {
                self.at = None;
                if let Some(r) = held(p.pool(), s) {
                    self.pick(p, part, r);
                }
            }
            SourceRow::Card { stem, .. } => {
                let size = self.size(stem).unwrap_or_default();
                self.at = Some((part, stem, size));
                if self.heads.get(stem).is_none() {
                    self.job = Some(SourceJob::Head {
                        part,
                        stem,
                        size,
                        pick: false,
                    });
                }
            }
            SourceRow::Divider => {}
        }
    }

    /// `part` plays pool source `r`: the memory turned, the head's ROOT
    /// and PERIOD copied, read first if it isn't cached.
    fn pick(&mut self, p: &mut Project, part: PartId, r: SourceRef) {
        if p.part(part).sound.source_ref() == Some(r) {
            return;
        }
        let head = self.heads.get(r.crc);
        self.memo.turn(p.edit_part(part).sound, r, head.as_ref());
        if let (None, Some(size)) = (head, self.size(r.crc)) {
            self.job = Some(SourceJob::Head {
                part,
                stem: r.crc,
                size,
                pick: true,
            });
        }
    }

    /// MIX + PLUS on SOURCE: loads the card row it's on, if it fits:
    /// whole, or too big for the arena as its heads (spec § 5.6), or says
    /// why the Part needs it whole.
    pub fn load(&mut self, p: &mut Project, part: PartId) {
        let Some((_, stem, size)) = self.at.filter(|a| a.0 == part) else {
            return;
        };
        match self.kind(p, part, stem) {
            #[cfg(feature = "tape")]
            Some((head, LoadKind::Streamed(heads))) => {
                if let Some(l) = StreamLoad::start(p, head, heads) {
                    self.job = Some(SourceJob::Stream(part, l));
                }
            }
            Some((_, LoadKind::Refused(n))) => self.note = Some(needs_ram_words(n)),
            _ => {
                if let Some(l) = SourceLoad::start(p, stem, size) {
                    self.job = Some(SourceJob::Load(part, l));
                }
            }
        }
    }

    /// How card row `stem` loads for `part`, its head once read, with the
    /// START and END the pick will give it (`SourceMemo::turn`). Whole
    /// only without TAPE.
    fn kind(&self, p: &Project, part: PartId, stem: u32) -> Option<(SourceHead, LoadKind)> {
        if !cfg!(feature = "tape") {
            return None;
        }
        let head = self.heads.get(stem)?;
        let sound = &p.part(part).sound;
        let mut t: TapeEngineParams = sound.params.tape;
        if sound.source_ref() != Some(head.sref) {
            (t.start, t.end) = self.memo.get(stem).map_or((0.0, 1.0), |m| (m.start, m.end));
        }
        Some((head, load_kind(&head, p.pool().free(), &t)))
    }

    /// One pass of the queued card work. Out of line: its rows and the
    /// card's listing stay off the UI loop's frame.
    #[inline(never)]
    pub fn work<S: Store>(&mut self, card: &mut crate::storage::Card, s: &mut S, p: &mut Project) {
        let Some(job) = self.job.take() else { return };
        // A re-head runs unseen: its failures say nothing.
        #[cfg(feature = "tape")]
        let seen = !matches!(job, SourceJob::Rehead(_));
        #[cfg(not(feature = "tape"))]
        let seen = true;
        let ran = card.run(s, |s, r| {
            self.run(job, s, r, p);
            Ok::<_, StoreError>(())
        });
        if ran.is_err() && seen {
            // No card, or it won't mount: the job's own failure.
            self.note = Some(READ_FAILED);
        }
        // A re-head that found no card: tried again after another `REHEAD`.
        #[cfg(feature = "tape")]
        if ran.is_err() && !seen {
            self.wait = None;
        }
    }

    fn run<S: Store>(&mut self, job: SourceJob, s: &mut S, r: &Ready, p: &mut Project) {
        match job {
            SourceJob::List(part, delta) => {
                let listed = TapeList::list(s, r);
                self.card = Some(listed.unwrap_or(TapeList::EMPTY));
                self.note = listed.err().map(said);
                self.turn(p, part, delta);
            }
            SourceJob::Head {
                part,
                stem,
                size,
                pick,
            } => match read_head(s, r, stem, size) {
                Ok(h) => {
                    self.heads.put(h);
                    let sound = &p.part(part).sound;
                    if pick && sound.source_ref() == Some(h.sref) {
                        p.edit_part(part).sound.pick_source(&h, None);
                    }
                }
                Err(w) => self.note = Some(w),
            },
            SourceJob::Load(part, mut l) => match l.step(s, r, p) {
                LoadStep::More => self.job = Some(SourceJob::Load(part, l)),
                // Its head was read when the cell came onto its row.
                LoadStep::Done(_, h) => {
                    self.at = None;
                    self.pick(p, part, h.sref);
                }
                LoadStep::Failed(w) => self.note = Some(w),
            },
            #[cfg(feature = "tape")]
            SourceJob::Stream(part, mut l) => match l.step(s, r, p, STEP_BYTES as usize) {
                StreamStep::More => self.job = Some(SourceJob::Stream(part, l)),
                StreamStep::Done(_) => {
                    self.at = None;
                    self.pick(p, part, l.sref());
                }
                StreamStep::Failed(f) => self.note = Some(stream_fail_words(f)),
            },
            #[cfg(feature = "tape")]
            SourceJob::Rehead(mut l) => match l.step(s, r, p, STEP_BYTES as usize) {
                StreamStep::More => self.job = Some(SourceJob::Rehead(l)),
                StreamStep::Done(_) => {}
                // A card fault: tried again after another `REHEAD`.
                StreamStep::Failed(StreamFail::Read(e))
                    if crate::storage::CardFault::store_error(&e).is_some() =>
                {
                    self.wait = None
                }
                // Not again until the heads it wants change, or the card
                // (`Sounding` too: `can_rehead` let it start, so a retry
                // would only read the same heads again).
                StreamStep::Failed(_) => {
                    if let Some(w) = self.wait {
                        self.stuck[w.id.index()] = heads_key(&w.heads);
                    }
                }
            },
            // `UiState::lib_step`'s, not this.
            SourceJob::Stage(l) => self.job = Some(SourceJob::Stage(l)),
        }
    }
}

/// MIX + PLUS's words on a card row that fits: `LOADS · 41K`.
pub fn loads_words(cost: Cost) -> crate::project::Line {
    let mut l = crate::project::Line::new("LOADS · ");
    let _ = core::fmt::Write::write_str(&mut l, cost.kib_label().as_str());
    l
}

/// A card row that doesn't fit: `WON'T FIT · NEEDS 41K, 12K FREE`.
pub fn wont_fit_words(cost: Cost, free: Cost) -> crate::project::Line {
    let mut l = crate::project::Line::new("WON'T FIT · ");
    let needs = crate::project::PoolFull { needs: cost, free };
    let _ = core::fmt::Write::write_str(
        &mut l,
        crate::ui::settings::prompt::pool_needs(needs).as_str(),
    );
    l
}

pub const LOAD_KEYS: &str = "MIX + PLUS";

/// A label's tracking: the house's, or none where that runs past the
/// measure (WON'T FIT with 3-digit sizes on both sides).
pub fn tracking(text: &str) -> i32 {
    use crate::ui::{draw, theme};
    let t = theme::LABEL_TRACKING;
    let fits = draw::text_width(&theme::FONT_LABEL, text, t) <= theme::CONTENT_W;
    if fits { t } else { 0 }
}
/// The meter's top on SRC, under the band's label.
pub const METER_Y: i32 = crate::ui::theme::FOCUS_LABEL_Y + 8;

/// A card row's band: the meter with the row's cost previewed, and the
/// hint (ADR 0086), or why it won't fit in `ALERT`.
#[inline(never)]
pub fn draw_card_row<D>(d: &mut D, pool: &Pool, cost: Cost, free: Cost, hints: bool)
where
    D: embedded_graphics::draw_target::DrawTarget<Color = embedded_graphics::pixelcolor::Rgb565>,
{
    use crate::ui::{draw, hint, meter, theme};
    meter::draw(
        d,
        &meter::MeterView::of(pool, Some(cost)),
        theme::MARGIN_X,
        METER_Y,
    );
    if !hints {
        return;
    }
    if cost <= free {
        let words = loads_words(cost);
        let g = hint::Gesture(LOAD_KEYS, words.as_str());
        hint::gesture(d, &g, theme::MARGIN_X, hint::FOCUS_Y, theme::MID);
    } else {
        let words = wont_fit_words(cost, free);
        let t = tracking(words.as_str());
        let (x, y) = (theme::MARGIN_X, hint::FOCUS_Y);
        draw::text_tracked(d, &theme::FONT_LABEL, words.as_str(), x, y, theme::ALERT, t);
    }
}
