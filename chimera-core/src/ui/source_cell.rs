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
use crate::dsp::tape_engine::read::Shape;
#[cfg(feature = "tape")]
use crate::dsp::tape_engine::read::head_table;
use crate::dsp::tape_engine::{NeedsRam, Seeks};
use crate::preset::Sound;
use crate::project::arena::{Cost, MAX_SOURCES};
use crate::project::{
    Absence, PartId, Pool, Project, Refusal, SOURCE_HEAD, SourceId, SourceNeed, StreamFail,
};
pub use crate::project::{LoadKind, load_kind};
#[cfg(feature = "tape")]
use crate::project::{StreamLoad, StreamStep};
use crate::storage::{
    Card, CardEvent, MAX_TAPES, Ready, SourceCheck, SourceHead, SourceRef, TapeList, TapeSource,
    after_mount, held_for_file_len, source_name,
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
/// de-frame, so its voices drop out over them. Its cell reads `BAD FILE`
/// (`DAMAGED` is a pixel past the column). The band promises no fix: `put`
/// won't replace a sealed damaged file (ADR 0106, #267).
pub const FILE_DAMAGED: &str = "BAD FILE · DROPS OUT";
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
        NeedsRam::Machine(Seeks::Scrub) => "SCRUB NEEDS RAM",
        NeedsRam::Machine(Seeks::Stretch) => "STRETCH NEEDS RAM",
        NeedsRam::Machine(Seeks::Wave) => "WAVE NEEDS RAM",
    }
}

/// Why a card file too big for the arena won't load: the focus band's
/// words (ADR 0186).
pub fn refusal_words(r: Refusal) -> &'static str {
    match r {
        Refusal::NeedsRam(n) => needs_ram_words(n),
        Refusal::Shape(Shape::Transwave) => TRANSWAVE_NO_STREAM,
        // A plain source streams: never refused for its shape (`load_kind`).
        Refusal::Shape(Shape::ZoneSet | Shape::Plain) => ZONES_NO_STREAM,
        Refusal::TooShort => TOO_SHORT_TO_STREAM,
        Refusal::Closed => crate::bank2::NEEDS_UPDATE,
    }
}

/// `refusal_words`, short enough for the cell's column.
pub fn refusal_cell(r: Refusal) -> &'static str {
    match r {
        Refusal::NeedsRam(_) => "RAM",
        Refusal::Shape(_) => "TOO BIG",
        Refusal::TooShort => "SHORT",
        Refusal::Closed => crate::bank2::NEEDS_UPDATE_CELL,
    }
}

pub const TRANSWAVE_NO_STREAM: &str = "TRANSWAVE: NO STREAM";
pub const ZONES_NO_STREAM: &str = "ZONES: NO STREAM";
pub const TOO_SHORT_TO_STREAM: &str = "TOO SHORT TO STREAM";

/// Why SOURCE's card work failed: the focus band's words until the next
/// input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceNote {
    NotOnCard,
    ReadFailed,
    Fragmented,
    StreamsFull,
    PoolFull,
    Refused(Refusal),
}

impl SourceNote {
    pub fn words(self) -> &'static str {
        match self {
            SourceNote::NotOnCard => NOT_ON_CARD,
            SourceNote::ReadFailed => READ_FAILED,
            SourceNote::Fragmented => FRAGMENTED,
            SourceNote::StreamsFull => STREAMS_FULL,
            SourceNote::PoolFull => crate::ui::settings::prompt::POOL_FULL,
            SourceNote::Refused(r) => refusal_words(r),
        }
    }
}

/// Why a streamed load published nothing.
pub fn stream_note(f: StreamFail) -> SourceNote {
    match f {
        StreamFail::Fragmented => SourceNote::Fragmented,
        StreamFail::Full => SourceNote::StreamsFull,
        StreamFail::Pool(_) => SourceNote::PoolFull,
        StreamFail::Read(crate::storage::SourceReadError::Load(
            crate::storage::LoadError::Missing,
        )) => SourceNote::NotOnCard,
        StreamFail::Read(_) | StreamFail::Sounding => SourceNote::ReadFailed,
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

/// What a card read's failure reads: a file gone, or anything else.
pub fn said(e: StoreError) -> SourceNote {
    match e {
        StoreError::NotFound => SourceNote::NotOnCard,
        _ => SourceNote::ReadFailed,
    }
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
pub fn read_head(
    s: &mut dyn Store,
    r: &Ready,
    stem: u32,
    size: u32,
) -> Result<SourceHead, SourceNote> {
    let mut check = SourceCheck::new(stem, size).map_err(|_| SourceNote::ReadFailed)?;
    let mut feed = Feed {
        check: &mut check,
        held: None,
        left: STEP_BYTES,
        got: 0,
        refused: false,
    };
    s.read_at(r.volume(), source_name(stem), 0, &mut feed)
        .map_err(said)?;
    check.head().ok_or(SourceNote::ReadFailed)
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
    Failed(SourceNote),
}

/// One pass of a `SourceLoad` that doesn't publish.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StagedStep {
    More,
    /// Whole and passing in the staging, unpublished.
    Staged(SourceNeed),
    Failed(SourceNote),
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
    pub fn step(&mut self, s: &mut dyn Store, r: &Ready, p: &mut Project) -> LoadStep {
        match self.step_staged(s, r, p) {
            StagedStep::More => LoadStep::More,
            StagedStep::Failed(w) => LoadStep::Failed(w),
            StagedStep::Staged(need) => {
                match (p.source_commit(need.r, need.held), self.check.head()) {
                    (Ok(id), Some(head)) => LoadStep::Done(id, head),
                    _ => LoadStep::Failed(SourceNote::PoolFull),
                }
            }
        }
    }

    /// `step` without the commit: the last chunk leaves the whole, passing
    /// file staged, unpublished, and says what it is. A library load's
    /// `adopt` publishes it.
    pub fn step_staged(&mut self, s: &mut dyn Store, r: &Ready, p: &mut Project) -> StagedStep {
        // A gap that moved under the staging: never published.
        let Some((stage, held)) = p.source_staging(self.held) else {
            return StagedStep::Failed(SourceNote::ReadFailed);
        };
        if stage != self.stage {
            return StagedStep::Failed(SourceNote::ReadFailed);
        }
        let mut feed = Feed {
            check: &mut self.check,
            held: Some(held),
            left: STEP_BYTES,
            got: 0,
            refused: false,
        };
        let read = s.read_at(r.volume(), source_name(self.stem), self.at, &mut feed);
        let (got, refused) = (feed.got, feed.refused);
        if let Err(e) = read {
            return StagedStep::Failed(said(e));
        }
        if refused || got == 0 {
            return StagedStep::Failed(SourceNote::ReadFailed);
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
            _ => StagedStep::Failed(SourceNote::ReadFailed),
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
pub(crate) enum SourceJob {
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

mod card;

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
    note: Option<SourceNote>,
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
        crate::bank2::ui_door(|u| u.source_tick(self, p));
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
        // Closed, the cell can't turn or load: it says so (ADR 0175).
        if crate::bank2::ui_closed() {
            return SourceState::Failed(SourceNote::Refused(Refusal::Closed));
        }
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

    /// SOURCE turned `delta` detents on `part`: a pool row plays at once;
    /// a card row is only shown, dimmed, until MIX + PLUS loads it
    /// (`card`'s `turn_in`). Closed, it doesn't turn.
    pub fn turn(&mut self, p: &mut Project, part: PartId, delta: i8) {
        crate::bank2::ui_door(|u| u.source_turn(self, p, part, delta));
    }

    /// MIX + PLUS on SOURCE: loads the card row it's on, if it fits
    /// (`card`'s `load_in`). Closed, nothing loads.
    pub fn load(&mut self, p: &mut Project, part: PartId) {
        crate::bank2::ui_door(|u| u.source_load(self, p, part));
    }

    /// How card row `stem` loads for `part` (`card`'s `kind_in`); closed,
    /// unknown.
    fn kind(&self, p: &Project, part: PartId, stem: u32) -> Option<(SourceHead, LoadKind)> {
        crate::bank2::ui_door(|u| u.source_kind(self, p, part, stem)).flatten()
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
            self.note = Some(SourceNote::ReadFailed);
        }
        // A re-head that found no card: tried again after another `REHEAD`.
        #[cfg(feature = "tape")]
        if ran.is_err() && !seen {
            self.wait = None;
        }
    }

    /// `job` on the card (`card`'s `run_in`); a library load's source is
    /// `UiState::lib_step`'s, never run here. Closed, the job is dropped.
    fn run(&mut self, job: SourceJob, s: &mut dyn Store, r: &Ready, p: &mut Project) {
        match job {
            SourceJob::Stage(l) => self.job = Some(SourceJob::Stage(l)),
            job => {
                crate::bank2::ui_door(move |u| u.source_run(self, job, s, r, p));
            }
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
