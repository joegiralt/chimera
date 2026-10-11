//! The Sound library on the card (library spec § Save): a Part's Sound to
//! `/CHIMERA/SOUNDS/S#######.A`, stamped with the build.

use core::ops::ControlFlow;

use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError, VolumeId};

use super::arena::Cost;
use super::guard::Confirmed;
use super::sources::source_cost;
use super::store::{CardOut, MAX_LISTED, split};
use super::{
    Absence, PartFrom, PartId, PartSource, PoolFull, Project, ReplaceError, SlotId, SourceRef,
};
use crate::name::SoundName;
use crate::params::EngineType;
use crate::preset::Sound;
use crate::storage::{
    BuildStamp, Card, CardFault, FileError, Generation, HEADER_LEN, ID_WINDOW, LIB_PEEK, LibId,
    LibraryDecoder, LoadError, NextId, Peek, SaveError, Side, SideState, SoundCheck, check_file,
    encode_library_sound, held_for_file_len, lib_id, library_file, load_ab_in_place, next_user_id,
    peek_library, save_ab, sound_crc, source_file,
};

/// What a library operation says to the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LibNote {
    Store(StoreError),
    File(FileError),
    NoIds,
    /// A listed file is not what the card has now.
    Changed,
    Gone,
}

impl LibNote {
    pub fn message(self) -> &'static str {
        match self {
            LibNote::Store(e) => e.message(),
            LibNote::File(e) => e.message(),
            LibNote::NoIds => "NO SOUND IDS LEFT",
            LibNote::Changed => "FILE CHANGED",
            LibNote::Gone => "FILE IS GONE",
        }
    }
}

impl From<StoreError> for LibNote {
    fn from(e: StoreError) -> Self {
        LibNote::Store(e)
    }
}

impl CardFault for LibNote {
    fn store_error(&self) -> Option<StoreError> {
        match self {
            LibNote::Store(e) => Some(*e),
            _ => None,
        }
    }
}

/// The user ids in `[base, base + ID_WINDOW)` there are, as a bitmap.
fn window<S: Store>(
    s: &mut S,
    r: &crate::storage::Ready,
    base: u32,
) -> Result<[u8; 512], StoreError> {
    let mut taken = [0; 512];
    s.list(r.volume(), Dir::Sounds, &mut |f, _| {
        if let Some(id) = lib_id(f)
            && let Some(i) = id.get().checked_sub(base).filter(|&i| i < ID_WINDOW)
        {
            taken[i as usize / 8] |= 1 << (i % 8);
        }
    })?;
    Ok(taken)
}

/// The next free user id: `top + 1` from one pass, else the lowest gap.
fn free_id<S: Store>(s: &mut S, r: &crate::storage::Ready) -> Result<LibId, LibNote> {
    let mut top = 0;
    s.list(r.volume(), Dir::Sounds, &mut |f, _| {
        if let Some(id) = lib_id(f).filter(|id| !id.is_factory()) {
            top = top.max(id.get());
        }
    })?;
    let mut base = 1;
    loop {
        let taken = if top < LibId::USER_MAX {
            [0; 512]
        } else {
            window(s, r, base)?
        };
        match next_user_id(top, base, &taken) {
            NextId::Id(id) => return Ok(id),
            NextId::Scan(b) => base = b,
            NextId::NoIds => return Err(LibNote::NoIds),
        }
    }
}

/// `s` as a new library file named `name`: the directories made first, the
/// id the next free one, the file read back whole. Any failure after the
/// write began deletes it.
pub fn save_library_sound<S: Store>(
    card: &mut Card,
    store: &mut S,
    s: &Sound,
    name: SoundName,
    stamp: &BuildStamp,
) -> CardOut<Result<LibId, LibNote>> {
    let run = card.run(store, |st, r| {
        st.make_dir(r.volume(), Dir::Chimera)?;
        st.make_dir(r.volume(), Dir::Sounds)?;
        let id = free_id(st, r)?;
        let file = library_file(id);
        let saved = save_ab(st, r, file, &mut SoundCheck::new(), Some(name), &mut |w| {
            encode_library_sound(s, stamp, None, w)
        })
        .map_err(|e| match e {
            SaveError::Store(e) => LibNote::Store(e),
            SaveError::File(e) => LibNote::File(e),
            SaveError::Damaged => LibNote::File(FileError::Corrupt),
        })
        .and_then(|_| {
            match check_file(st, r, file.side(Side::A), &mut SoundCheck::new())? {
                SideState::Present { err: None, .. } => Ok(()),
                SideState::Torn(e)
                | SideState::Headerless(e)
                | SideState::Present { err: Some(e), .. } => Err(LibNote::File(e)),
                SideState::Missing => Err(LibNote::File(FileError::Truncated)),
            }
        });
        if saved.is_err() {
            // Best effort: nothing else names this id.
            let _ = st.delete(r.volume(), file.side(Side::A));
        }
        saved.map(|()| id)
    });
    let (out, event) = split(run);
    CardOut { out, event }
}

/// What a listing saw of a file: the witness `FILE CHANGED` checks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LibSeen {
    pub generation: Option<Generation>,
    pub name: Option<SoundName>,
}

/// A listed file, as the card had it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LibFile {
    pub id: LibId,
    pub vol: VolumeId,
    pub seen: LibSeen,
}

/// One row: the newer readable side's name, engine and stamp, or the
/// error that left it unreadable. Its volume is the page's.
#[derive(Clone, Copy, Debug)]
pub struct LibEntry {
    pub id: LibId,
    pub seen: LibSeen,
    pub engine: Option<EngineType>,
    pub stamp: Option<BuildStamp>,
    /// That side's size.
    pub len: u32,
    pub err: Option<FileError>,
}

/// Which page: the newest, or the 48 just below / above an id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageAt {
    Newest,
    Below(LibId),
    Above(LibId),
}

/// One page of the library, user ids highest first.
pub struct LibPage {
    entries: [Option<LibEntry>; MAX_LISTED],
    len: u8,
    pub newer: bool,
    pub older: bool,
    pub vol: Option<VolumeId>,
}

impl LibPage {
    pub const EMPTY: LibPage = LibPage {
        entries: [None; MAX_LISTED],
        len: 0,
        newer: false,
        older: false,
        vol: None,
    };

    pub fn entries(&self) -> &[Option<LibEntry>] {
        &self.entries[..usize::from(self.len)]
    }

    pub fn position(&self, id: LibId) -> Option<usize> {
        self.entries()
            .iter()
            .position(|e| e.is_some_and(|e| e.id == id))
    }

    /// `e`, a row of this page, as the file a load re-checks.
    pub fn file(&self, e: &LibEntry) -> Option<LibFile> {
        Some(LibFile {
            id: e.id,
            vol: self.vol?,
            seen: e.seen,
        })
    }
}

/// A listed id and the sizes of its two sides.
type Sized = (LibId, [Option<u32>; 2]);

/// What one directory pass found: the `MAX_LISTED` user ids nearest `at`,
/// ascending, with their sides' sizes.
struct Near {
    ids: [Sized; MAX_LISTED],
    len: usize,
    /// Below: keep the highest. Above: keep the lowest.
    high: bool,
    /// Ids beyond the kept ones, on the far side from `at`.
    more: bool,
    /// An id on `at`'s near side or equal to it: the page before exists.
    back: bool,
}

impl Near {
    fn see(&mut self, at: PageAt, id: LibId, side: usize, size: u32) {
        let (lo, hi) = match at {
            PageAt::Newest => (0, LibId::USER_MAX + 1),
            PageAt::Below(x) => (0, x.get()),
            PageAt::Above(x) => (x.get() + 1, LibId::USER_MAX + 1),
        };
        let n = id.get();
        if !(lo..hi).contains(&n) {
            self.back |= match at {
                PageAt::Below(_) => n >= hi,
                PageAt::Above(_) => n < lo,
                PageAt::Newest => false,
            };
            return;
        }
        match self.ids[..self.len].binary_search_by_key(&id, |s| s.0) {
            Ok(i) => self.ids[i].1[side] = Some(size),
            Err(at) => {
                let mut sizes = [None; 2];
                sizes[side] = Some(size);
                if self.len == MAX_LISTED {
                    self.more = true;
                    if self.high {
                        if at == 0 {
                            return;
                        }
                        self.ids.copy_within(1..at, 0);
                        self.ids[at - 1] = (id, sizes);
                    } else {
                        if at == MAX_LISTED {
                            return;
                        }
                        self.ids.copy_within(at..MAX_LISTED - 1, at + 1);
                        self.ids[at] = (id, sizes);
                    }
                } else {
                    self.ids.copy_within(at..self.len, at + 1);
                    self.ids[at] = (id, sizes);
                    self.len += 1;
                }
            }
        }
    }
}

/// A side's first `LIB_PEEK` bytes, then a break.
struct PeekSink {
    buf: [u8; LIB_PEEK],
    len: usize,
}

impl ReadSink for PeekSink {
    fn begin(&mut self, _: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
        let n = b.len().min(LIB_PEEK - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&b[..n]);
        self.len += n;
        if self.len == LIB_PEEK {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// One side's peek; `None` when it vanished since the pass. Under a
/// header and a record it is `Truncated` with no read.
fn peek_side<S: Store>(
    s: &mut S,
    vol: VolumeId,
    f: FileName,
    size: u32,
) -> Result<Option<Result<Peek, FileError>>, StoreError> {
    if (size as usize) < HEADER_LEN + 4 {
        return Ok(Some(Err(FileError::Truncated)));
    }
    let mut sink = PeekSink {
        buf: [0; LIB_PEEK],
        len: 0,
    };
    match s.read(vol, f, &mut sink) {
        Ok(()) => {}
        Err(StoreError::NotFound) => return Ok(None),
        Err(StoreError::Corrupt) => return Ok(Some(Err(FileError::Truncated))),
        Err(e) => return Err(e),
    }
    Ok(Some(peek_library(&sink.buf[..sink.len], size)))
}

/// An id's row from its sides, `project::store::entry`'s rule: a side that
/// needs newer firmware is the error even beside a readable one; else the
/// newer readable side; else the first error.
fn lib_entry(id: LibId, sides: [Option<(Result<Peek, FileError>, u32)>; 2]) -> LibEntry {
    let mut best: Option<(&Peek, u32)> = None;
    let mut err = None;
    let mut nnf = false;
    for (r, len) in sides.iter().flatten() {
        match r {
            Ok(p) => {
                nnf |= p.needs_newer;
                if best.is_none_or(|(b, _)| p.generation.is_newer_than(b.generation)) {
                    best = Some((p, *len));
                }
            }
            Err(e) => {
                nnf |= *e == FileError::NeedsNewerFirmware;
                err = err.or(Some(*e));
            }
        }
    }
    LibEntry {
        id,
        seen: LibSeen {
            generation: best.map(|(p, _)| p.generation),
            name: best.and_then(|(p, _)| p.name),
        },
        engine: best.and_then(|(p, _)| p.engine),
        stamp: best.and_then(|(p, _)| p.stamp),
        len: best.map_or(0, |(_, l)| l),
        err: if nnf {
            Some(FileError::NeedsNewerFirmware)
        } else if best.is_some() {
            None
        } else {
            err
        },
    }
}

/// An id's row from the sides of the sizes listed: the one rule the
/// listing and a load's re-check share. A side that vanished since is
/// skipped; with neither read, no generation and no error.
fn peek_pair<S: Store>(
    s: &mut S,
    vol: VolumeId,
    id: LibId,
    sizes: [Option<u32>; 2],
) -> Result<LibEntry, StoreError> {
    let file = library_file(id);
    let mut sides = [None, None];
    for (i, side) in [Side::A, Side::B].into_iter().enumerate() {
        if let Some(size) = sizes[i]
            && let Some(p) = peek_side(s, vol, file.side(side), size)?
        {
            sides[i] = Some((p, size));
        }
    }
    Ok(lib_entry(id, sides))
}

/// The page of user Sounds at `at`, highest id first, into `out` in place:
/// one directory pass, then a peek of each seen side of the kept ids.
pub fn list_library<S: Store>(
    card: &mut Card,
    store: &mut S,
    at: PageAt,
    out: &mut LibPage,
) -> CardOut<Result<(), LibNote>> {
    out.len = 0;
    out.newer = false;
    out.older = false;
    out.vol = None;
    let run = card.run(store, |s, r| {
        let vol = r.volume();
        out.vol = Some(vol);
        let mut near = Near {
            ids: [(LibId::new(1).unwrap(), [None; 2]); MAX_LISTED],
            len: 0,
            high: !matches!(at, PageAt::Above(_)),
            more: false,
            back: false,
        };
        match s.list(vol, Dir::Sounds, &mut |f, size| {
            if let Some(id) = lib_id(f).filter(|id| !id.is_factory()) {
                let side = usize::from(f.ext() == b"B");
                near.see(at, id, side, size);
            }
        }) {
            Ok(()) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        for &(id, sizes) in near.ids[..near.len].iter().rev() {
            out.entries[usize::from(out.len)] = Some(peek_pair(s, vol, id, sizes)?);
            out.len += 1;
        }
        (out.newer, out.older) = match at {
            PageAt::Newest => (false, near.more),
            PageAt::Below(_) => (near.back, near.more),
            PageAt::Above(_) => (near.more, near.back),
        };
        Ok(())
    });
    let (out, event) = split(run);
    CardOut { out, event }
}

/// EDIT on a library row: the Part it replaces, and the file as listed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LibraryPart {
    pub part: PartId,
    pub file: LibFile,
}

/// A TAPE source a read Sound needs and the pool doesn't hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceNeed {
    pub r: SourceRef,
    /// From its `.SRC`'s directory size; 0 when not on the card, or on it
    /// at a length no whole file has (damaged).
    pub held: usize,
    pub on_card: bool,
}

impl SourceNeed {
    /// A `.SRC` to read: on the card with bytes. Anything else is stored
    /// absent, at no cost.
    pub fn readable(&self) -> bool {
        self.on_card && self.held > 0
    }

    /// Why it is stored absent: damaged on the card, or not there.
    fn absence(&self) -> Absence {
        if self.on_card {
            Absence::Missing
        } else {
            Absence::NotOnCard
        }
    }
}

/// A library Sound in the pool's scratch: what it lost, what it needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LibRead {
    pub skips: u16,
    pub source: Option<SourceNeed>,
}

/// The source `adopt` commits: staged bytes, or absent.
#[derive(Clone, Copy, Debug)]
pub enum Staged {
    Read(SourceNeed),
    Missing(SourceRef),
}

#[derive(Debug, PartialEq)]
pub struct Adopted {
    pub slot: SlotId,
    pub deduped: bool,
}

#[derive(Debug, PartialEq)]
pub enum AdoptError {
    /// The Part moved since it was confirmed.
    Changed,
    /// `None`: every slot taken.
    Full(Option<PoolFull>),
}

impl From<AdoptError> for ReplaceError {
    fn from(e: AdoptError) -> Self {
        match e {
            AdoptError::Changed => ReplaceError::Changed,
            AdoptError::Full(f) => ReplaceError::Full(f),
        }
    }
}

/// `source`'s pool need now: `None` when the pool holds it with its bytes.
fn source_need<S: Store>(
    s: &mut S,
    vol: VolumeId,
    p: &Project,
    r: SourceRef,
) -> Result<Option<SourceNeed>, StoreError> {
    let pool = &p.pool;
    if pool
        .find_source(r)
        .and_then(|id| pool.source(id))
        .is_some_and(|e| !e.absent())
    {
        return Ok(None);
    }
    let name = source_file(r);
    let mut size = None;
    match s.list(vol, Dir::Tapes, &mut |f, len| {
        if f == name {
            size = Some(len);
        }
    }) {
        Ok(()) | Err(StoreError::NotFound) => {}
        Err(e) => return Err(e),
    }
    Ok(Some(SourceNeed {
        r,
        held: size.and_then(held_for_file_len).map_or(0, |h| h as usize),
        on_card: size.is_some(),
    }))
}

/// `file` decoded into the pool's scratch, nothing else touched: its
/// sides re-peeked first, so a pair gone or re-written since the listing
/// refuses. A clobbered scratch is harmless: only `adopt` reads it.
pub fn read_library_sound<S: Store>(
    card: &mut Card,
    store: &mut S,
    p: &mut Project,
    file: LibFile,
) -> CardOut<Result<LibRead, LibNote>> {
    let run = card.run(store, |s, r| {
        let vol = r.volume();
        if vol != file.vol {
            return Ok(Err(LibNote::Store(StoreError::VolumeChanged(vol))));
        }
        let ab = library_file(file.id);
        let mut sizes = [None; 2];
        match s.list(vol, Dir::Sounds, &mut |f, size| {
            if lib_id(f) == Some(file.id) {
                sizes[usize::from(f.ext() == b"B")] = Some(size);
            }
        }) {
            Ok(()) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        let now = peek_pair(s, vol, file.id, sizes)?;
        if now.seen.generation.is_none() && now.err.is_none() {
            return Ok(Err(LibNote::Gone));
        }
        if now.seen != file.seen {
            return Ok(Err(LibNote::Changed));
        }
        let mut d = LibraryDecoder::new(p.pool.scratch());
        match load_ab_in_place(s, r, ab, &mut d) {
            // The newer side failing its CRC leaves the older: not the row.
            Ok(h) => {
                if Some(h.generation) != file.seen.generation || h.name != file.seen.name {
                    return Ok(Err(LibNote::Changed));
                }
            }
            Err(e) => {
                return match e.err {
                    LoadError::Store(e) => Err(e.into()),
                    LoadError::File(e) => Ok(Err(LibNote::File(e))),
                    // Busy: no `begin`, never.
                    LoadError::Missing | LoadError::Busy => Ok(Err(LibNote::Gone)),
                };
            }
        }
        let skips = d.skips();
        let source = match p.pool.scratch_ref().source_ref() {
            Some(sr) => source_need(s, vol, p, sr)?,
            None => None,
        };
        Ok(Ok(LibRead { skips, source }))
    });
    let (out, event) = split(run);
    CardOut {
        out: out.and_then(|r| r),
        event,
    }
}

impl Project {
    /// What adopting the scratch Sound takes: `Ok(Some(slot))` for a slot
    /// already holding it, `Ok(None)` when it fits. Its slot cost and a
    /// free Sound slot, plus, for a source on the card, its cost and a
    /// free source slot. A source not on the card is stored absent: no
    /// cost, no slot.
    pub fn library_needs(
        &self,
        src: Option<SourceNeed>,
    ) -> Result<Option<SlotId>, Option<PoolFull>> {
        let pool = &self.pool;
        let crc = sound_crc(pool.scratch_ref());
        let dedupe = SlotId::ALL
            .into_iter()
            .find(|&s| pool.get(s).is_some_and(|e| e.crc() == crc));
        let mut needs = Cost::default();
        if dedupe.is_none() {
            let slot = pool.first_free().ok_or(None)?;
            needs = needs + pool.needs(slot, pool.scratch_ref());
        }
        if let Some(src) = src.filter(SourceNeed::readable) {
            match pool.find_source(src.r).and_then(|id| pool.source(id)) {
                Some(e) if !e.absent() => {}
                found => {
                    if found.is_none() && pool.free_source().is_none() {
                        return Err(None);
                    }
                    needs = needs + source_cost(src.held);
                }
            }
        }
        if !pool.fits(needs) {
            return Err(Some(PoolFull {
                needs,
                free: pool.free(),
            }));
        }
        Ok(dedupe)
    }

    /// The scratch Sound into the first free slot (or the one holding it),
    /// with `staged`'s source, and the Part loaded from it: the witness
    /// re-checked, then the budget, before anything moves. A refusal
    /// changes nothing. Only a library load's confirmation adopts:
    ///
    /// ```compile_fail,E0308
    /// use chimera_engine::project::{PartSource, Project, ReplaceGuard, TemplateCrc};
    /// fn f(p: &mut Project, t: TemplateCrc, s: PartSource) {
    ///     if let Ok(c) = ReplaceGuard::check(p, t, s) {
    ///         let _ = p.adopt(c, None);
    ///     }
    /// }
    /// ```
    pub fn adopt(
        &mut self,
        c: Confirmed<LibraryPart>,
        staged: Option<Staged>,
    ) -> Result<Adopted, AdoptError> {
        if !c.holds(self) {
            return Err(AdoptError::Changed);
        }
        let need = staged.map(|s| match s {
            Staged::Read(n) => n,
            Staged::Missing(r) => SourceNeed {
                r,
                held: 0,
                on_card: false,
            },
        });
        let dedupe = self.library_needs(need).map_err(AdoptError::Full)?;
        match need {
            Some(n) if n.readable() => {
                self.pool
                    .source_commit(n.r, n.held)
                    .map_err(|f| AdoptError::Full(Some(f)))?;
            }
            // Absent; its slot, if every one is taken, refuses here:
            // nothing moved.
            Some(n) => {
                let id = self.pool.source_absent(n.r).ok_or(AdoptError::Full(None))?;
                self.pool.mark_absent(id, n.absence());
            }
            None => {}
        }
        let slot = match dedupe {
            Some(s) => s,
            None => {
                let s = self.pool.first_free().expect("library_needs found one");
                self.pool.store_scratch(s).expect("library_needs: it fits");
                s
            }
        };
        self.load_part(PartSource {
            part: c.target().part,
            from: PartFrom::Slot(slot),
        })
        .expect("a slot just checked or stored decodes");
        Ok(Adopted {
            slot,
            deduped: dedupe.is_some(),
        })
    }
}
