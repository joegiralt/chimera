//! The Sound library on the card (library spec § Save): a Part's Sound to
//! `/CHIMERA/SOUNDS/S#######.A`, stamped with the build.

use core::mem::MaybeUninit;
use core::ops::ControlFlow;
use core::ptr::addr_of_mut;

use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError, VolumeId};

use super::arena::Cost;
use super::guard::Confirmed;
use super::sources::source_cost;
use super::store::{CardOut, MAX_LISTED, split};
use super::{
    Absence, KitDoor, KitMark, KitPut, KitScan, LibraryKit, PartFrom, PartId, PartSource, PoolFull,
    Project, ReadKit, ReplaceError, SlotId, SourceRef,
};
use crate::dsp::drum::types::Kit;
use crate::midi_map::KeptMap;
use crate::name::SoundName;
use crate::params::EngineType;
use crate::preset::Sound;
use crate::storage::{
    BuildStamp, Card, CardFault, Check, DecodeInPlace, Event, FileError, FileKind, Generation,
    HEADER_LEN, ID_WINDOW, LIB_PEEK, LibId, LibKind, LibraryDecoder, LoadError, NextId, Note, Peek,
    RecordWriter, SaveError, Side, SideState, SoundCheck, UserId, check_file, encode_library_part,
    held_for_file_len, lib_id, library_file, load_ab_in_place, next_user_id, peek_library, save_ab,
    sound_crc, source_file, user_file,
};

/// What a library operation says to the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LibNote {
    Store(StoreError),
    File(FileError),
    NoIds,
    /// A DRUM Part's save with bank 2 closed: its kit can't be written,
    /// so nothing is (ADR 0305).
    KitClosed,
    /// A listed file is not what the card has now.
    Changed,
    Gone,
}

impl LibNote {
    #[optimize(size)]
    pub fn message(self) -> &'static str {
        match self {
            LibNote::Store(e) => e.message(),
            LibNote::File(e) => e.message(),
            LibNote::NoIds => "NO SOUND IDS LEFT",
            LibNote::KitClosed => "KIT NOT SAVED \u{b7} NEEDS UPDATE",
            LibNote::Changed => "FILE CHANGED",
            LibNote::Gone => "FILE IS GONE",
        }
    }
}

impl From<StoreError> for LibNote {
    #[optimize(size)]
    fn from(e: StoreError) -> Self {
        LibNote::Store(e)
    }
}

impl CardFault for LibNote {
    #[optimize(size)]
    fn store_error(&self) -> Option<StoreError> {
        match self {
            LibNote::Store(e) => Some(*e),
            _ => None,
        }
    }
}

/// The user ids in `[base, base + ID_WINDOW)` there are, as a bitmap.
#[optimize(size)]
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
#[optimize(size)]
fn free_id<S: Store>(s: &mut S, r: &crate::storage::Ready) -> Result<UserId, LibNote> {
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

/// What a library save writes of a Part (ADR 0305): a DRUM Part's Sound
/// goes with its kit, any other Sound alone. No other pairing exists.
#[derive(Clone, Copy, Debug)]
pub enum PartSave<'a> {
    Drum { sound: &'a Sound, kit: &'a Kit },
    Sound(&'a Sound),
}

impl<'a> PartSave<'a> {
    #[optimize(size)]
    fn sound(&self) -> &'a Sound {
        match *self {
            PartSave::Drum { sound, .. } | PartSave::Sound(sound) => sound,
        }
    }

    /// The kit's records after the Sound's, and what became of them.
    #[optimize(size)]
    fn put_kit(&self, w: &mut RecordWriter<'_>) -> Result<KitPut, StoreError> {
        Ok(match *self {
            PartSave::Sound(_) => KitPut::NotDrum,
            PartSave::Drum { kit, .. } => match KitDoor::put(kit, w)? {
                Some(crc) => KitPut::Written(crc),
                None => KitPut::Closed,
            },
        })
    }
}

impl Project {
    /// What Part `id`'s library save writes: by its engine, nothing else.
    #[optimize(size)]
    pub fn part_save(&self, id: PartId) -> PartSave<'_> {
        let sound: &Sound = &self.part(id).sound;
        match sound.engine() {
            EngineType::Drum => PartSave::Drum {
                sound,
                kit: self.kits.kit(id),
            },
            EngineType::Algo | EngineType::Modal | EngineType::Tape | EngineType::Va => {
                PartSave::Sound(sound)
            }
        }
    }

    /// Part `p`'s Sound is in the library as it sounds: its shifts are
    /// its own values now, so CC 64 is the Sound as saved (ADR 0263), and
    /// the kit the file holds is the Part's mark (ADR 0305). Only a save
    /// makes a `SavedPart`.
    #[optimize(size)]
    pub fn library_saved(&mut self, saved: SavedPart) {
        self.bump();
        self.perf.parts[saved.part.index()].env = [0; 2];
        if let Some(mark) = saved.kit {
            self.kits.set_mark(saved.part, mark);
        }
    }
}

/// A Part's library save, done: the file's id, and the mark of the kit
/// it holds, if it holds one. `Project::library_saved` takes it.
#[must_use]
#[derive(Debug)]
pub struct SavedPart {
    part: PartId,
    id: UserId,
    kit: Option<KitMark>,
}

impl SavedPart {
    #[optimize(size)]
    pub fn id(&self) -> UserId {
        self.id
    }

    /// The mark of the kit the file holds; `None` for a Sound with none.
    #[optimize(size)]
    pub fn kit(&self) -> Option<KitMark> {
        self.kit
    }
}

/// Part `part`'s Sound as `save_library_sound` saves it, with the map
/// record its closed door kept (#862) and, for a DRUM Part, its kit
/// (ADR 0305). A DRUM Part with bank 2 closed is refused, nothing left
/// on the card: its file would hold no kit.
#[optimize(size)]
pub fn save_library_part<S: Store>(
    card: &mut Card,
    store: &mut S,
    p: &Project,
    part: PartId,
    name: SoundName,
    stamp: &BuildStamp,
) -> CardOut<Result<SavedPart, LibNote>> {
    let CardOut { out, event } = save_library(
        card,
        store,
        p.part_save(part),
        &p.kept[part.index()],
        name,
        stamp,
    );
    let out = out.map(|(id, put)| SavedPart {
        part,
        id,
        kit: match put {
            KitPut::Written(crc) => Some(KitMark::held(crc)),
            KitPut::NotDrum | KitPut::Closed => None,
        },
    });
    CardOut { out, event }
}

/// `s` as a new library file named `name`: the directories made first, the
/// id the next free one, the file read back whole. Any failure after the
/// write began deletes it. A Sound alone: no Part, so no kit.
#[optimize(size)]
pub fn save_library_sound<S: Store>(
    card: &mut Card,
    store: &mut S,
    s: &Sound,
    name: SoundName,
    stamp: &BuildStamp,
) -> CardOut<Result<UserId, LibNote>> {
    let CardOut { out, event } = save_library(
        card,
        store,
        PartSave::Sound(s),
        &KeptMap::EMPTY,
        name,
        stamp,
    );
    CardOut {
        out: out.map(|(id, _)| id),
        event,
    }
}

#[optimize(size)]
fn save_library<S: Store>(
    card: &mut Card,
    store: &mut S,
    save: PartSave<'_>,
    kept: &KeptMap,
    name: SoundName,
    stamp: &BuildStamp,
) -> CardOut<Result<(UserId, KitPut), LibNote>> {
    let run = card.run(store, |st, r| {
        st.make_dir(r.volume(), Dir::Chimera)?;
        st.make_dir(r.volume(), Dir::Sounds)?;
        let id = free_id(st, r)?;
        let file = user_file(id);
        let mut put = KitPut::NotDrum;
        let saved = save_ab(st, r, file, &mut PartCheck::new(), Some(name), &mut |w| {
            encode_library_part(save.sound(), kept, stamp, w)?;
            put = save.put_kit(w)?;
            Ok(())
        })
        .map_err(|e| match e {
            SaveError::Store(e) => LibNote::Store(e),
            SaveError::File(e) => LibNote::File(e),
            SaveError::Damaged => LibNote::File(FileError::Corrupt),
        })
        .and_then(|_| {
            // The kit's records too, read back (`PartCheck`).
            match check_file(st, r, file.side(Side::A), &mut PartCheck::new())? {
                SideState::Present { err: None, .. } => Ok(()),
                SideState::Torn(e)
                | SideState::Headerless(e)
                | SideState::Present { err: Some(e), .. } => Err(LibNote::File(e)),
                SideState::Missing => Err(LibNote::File(FileError::Truncated)),
            }
        });
        let saved = match (saved, put) {
            (Ok(()), KitPut::Closed) => Err(LibNote::KitClosed),
            (saved, KitPut::Closed | KitPut::Written(_) | KitPut::NotDrum) => saved,
        };
        if saved.is_err() {
            // Best effort: nothing else names this id.
            let _ = st.delete(r.volume(), file.side(Side::A));
        }
        saved.map(|()| (id, put))
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
    /// The file's engine as the listing peeked it; `None` unknown. A
    /// load's guard reads it: a DRUM Sound brings a kit (ADR 0305).
    pub engine: Option<EngineType>,
}

/// One row: the newer readable side's name, engine and stamp, or the
/// error that left it unreadable. Its volume is the page's.
#[derive(Clone, Copy, Debug)]
pub struct LibEntry {
    pub id: LibId,
    pub seen: LibSeen,
    pub engine: Option<EngineType>,
    /// A user Sound's stamp (LIBRARY's line 3 compares it); a factory row
    /// keeps none, its line 3 the page's one set stamp. Its description is
    /// peeked for the selected row alone (`peek_note`).
    stamp: Option<BuildStamp>,
    /// That side's size.
    pub len: u32,
    pub err: Option<FileError>,
}

impl LibEntry {
    #[optimize(size)]
    pub fn stamp(&self) -> Option<BuildStamp> {
        self.stamp
    }
}

/// Which ids a page lists, and in what order: the player's newest first,
/// the factory's lowest (the curated order) first. Each by `LibId::kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shelf {
    User,
    Factory,
}

impl Shelf {
    /// `id`'s place down this shelf's pages, if it is on it.
    #[optimize(size)]
    fn rank(self, id: LibId) -> Option<u32> {
        match (self, id.kind()) {
            (Shelf::User, LibKind::User(u)) => Some(LibId::USER_MAX - u.get()),
            (Shelf::Factory, LibKind::Factory(f)) => Some(f.get()),
            _ => None,
        }
    }
}

/// Which page, in the shelf's order: the first, or the 48 just after /
/// before an id.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PageAt {
    First,
    After(LibId),
    Before(LibId),
}

/// One page of a shelf, in its order.
pub struct LibPage {
    entries: [Option<LibEntry>; MAX_LISTED],
    len: u8,
    /// A page before this one, and one after.
    pub prev: bool,
    pub next: bool,
    pub shelf: Shelf,
    /// FACTORY's set stamp: its first described row's build (a stray
    /// file from another set, as TAPE's, has no description).
    pub set_stamp: Option<BuildStamp>,
    pub vol: Option<VolumeId>,
}

// `init_in_place` writes each field: a new one fails to build here first.
crate::in_place::field_list!(LibPage => LibPage { entries, len, prev, next, shelf, set_stamp, vol });

impl LibPage {
    pub const EMPTY: LibPage = LibPage {
        entries: [None; MAX_LISTED],
        len: 0,
        prev: false,
        next: false,
        shelf: Shelf::User,
        set_stamp: None,
        vol: None,
    };

    /// `EMPTY`, written in place a row at a time: a `const` page would put
    /// its 48-row image in flash for the copy (about 3.8 KB).
    #[optimize(size)]
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` comes from `slot`, so it is non-null, aligned for
        // `LibPage` and exclusively ours. `addr_of_mut!` takes each field's
        // place without making a reference, so no `&`/`&mut` to the page or
        // a field exists before `assume_init_mut`. The rows' array is
        // `[Option<LibEntry>; MAX_LISTED]`: cast to its element type it is
        // aligned, and `add(i)` for `i < MAX_LISTED` stays inside it. Every
        // field (each of the 48 rows, `len`, `prev`, `next`, `shelf`,
        // `set_stamp`, `vol`, all `field_list!` above names) is written
        // exactly once with a valid value; padding needs no init. A `const` page written whole is the safe
        // form, but it copies a 48-row image kept in flash.
        unsafe {
            let rows = addr_of_mut!((*p).entries).cast::<Option<LibEntry>>();
            for i in 0..MAX_LISTED {
                rows.add(i).write(None);
            }
            addr_of_mut!((*p).len).write(0);
            addr_of_mut!((*p).prev).write(false);
            addr_of_mut!((*p).next).write(false);
            addr_of_mut!((*p).shelf).write(Shelf::User);
            addr_of_mut!((*p).set_stamp).write(None);
            addr_of_mut!((*p).vol).write(None);
            slot.assume_init_mut()
        }
    }

    #[optimize(size)]
    pub fn entries(&self) -> &[Option<LibEntry>] {
        &self.entries[..usize::from(self.len)]
    }

    /// Only the rows `keep` takes, in order.
    #[optimize(size)]
    pub fn retain(&mut self, keep: impl Fn(&LibEntry) -> bool) {
        let len = usize::from(self.len);
        let mut n = 0;
        for i in 0..len {
            let e = self.entries[i];
            if e.as_ref().is_none_or(&keep) {
                self.entries[n] = e;
                n += 1;
            }
        }
        self.entries[n..len].fill(None);
        // At most `len`, a `u8`.
        self.len = n as u8;
    }

    #[optimize(size)]
    pub fn position(&self, id: LibId) -> Option<usize> {
        self.entries()
            .iter()
            .position(|e| e.is_some_and(|e| e.id == id))
    }

    /// `e`, a row of this page, as the file a load re-checks.
    #[optimize(size)]
    pub fn file(&self, e: &LibEntry) -> Option<LibFile> {
        Some(LibFile {
            id: e.id,
            vol: self.vol?,
            seen: e.seen,
            engine: e.engine,
        })
    }
}

/// A listed id's rank on its shelf, the id, and the sizes of its two sides.
type Sized = (u32, LibId, [Option<u32>; 2]);

/// What one directory pass found: the `MAX_LISTED` ids nearest `at`, by
/// rank ascending, with their sides' sizes.
struct Near {
    ids: [Sized; MAX_LISTED],
    len: usize,
    /// Before: keep the highest ranks. Else: keep the lowest.
    high: bool,
    /// Ids beyond the kept ones, on the far side from `at`.
    more: bool,
    /// An id on `at`'s near side or equal to it: the page before exists.
    back: bool,
}

impl Near {
    /// `at` is its id's rank (`First`'s none).
    #[optimize(size)]
    fn see(&mut self, at: PageAt, at_rank: u32, n: u32, id: LibId, side: usize, size: u32) {
        let (lo, hi) = match at {
            PageAt::First => (0, u32::MAX),
            PageAt::After(_) => (at_rank.saturating_add(1), u32::MAX),
            PageAt::Before(_) => (0, at_rank),
        };
        if !(lo..hi).contains(&n) {
            self.back |= match at {
                PageAt::After(_) => n < lo,
                PageAt::Before(_) => n >= hi,
                PageAt::First => false,
            };
            return;
        }
        match self.ids[..self.len].binary_search_by_key(&n, |s| s.0) {
            Ok(i) => self.ids[i].2[side] = Some(size),
            Err(at) => {
                let mut sizes = [None; 2];
                sizes[side] = Some(size);
                let new = (n, id, sizes);
                if self.len == MAX_LISTED {
                    self.more = true;
                    if self.high {
                        if at == 0 {
                            return;
                        }
                        self.ids.copy_within(1..at, 0);
                        self.ids[at - 1] = new;
                    } else {
                        if at == MAX_LISTED {
                            return;
                        }
                        self.ids.copy_within(at..MAX_LISTED - 1, at + 1);
                        self.ids[at] = new;
                    }
                } else {
                    self.ids.copy_within(at..self.len, at + 1);
                    self.ids[at] = new;
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
    #[optimize(size)]
    fn begin(&mut self, _: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    #[optimize(size)]
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
#[optimize(size)]
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
/// newer readable side; else the first error. With it, that side's stamp,
/// which a factory row doesn't keep, and its description.
#[optimize(size)]
fn lib_entry(
    id: LibId,
    sides: [Option<(Result<Peek, FileError>, u32)>; 2],
) -> (LibEntry, Option<BuildStamp>, Option<Note>) {
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
    let stamp = best.and_then(|(p, _)| p.stamp);
    let e = LibEntry {
        id,
        seen: LibSeen {
            generation: best.map(|(p, _)| p.generation),
            name: best.and_then(|(p, _)| p.name),
        },
        engine: best.and_then(|(p, _)| p.engine),
        stamp: match id.kind() {
            LibKind::User(_) => stamp,
            LibKind::Factory(_) => None,
        },
        len: best.map_or(0, |(_, l)| l),
        err: if nnf {
            Some(FileError::NeedsNewerFirmware)
        } else if best.is_some() {
            None
        } else {
            err
        },
    };
    (e, stamp, best.and_then(|(p, _)| p.note))
}

/// An id's row from the sides of the sizes listed: the one rule the
/// listing and a load's re-check share. A side that vanished since is
/// skipped; with neither read, no generation and no error.
#[optimize(size)]
fn peek_pair<S: Store>(
    s: &mut S,
    vol: VolumeId,
    id: LibId,
    sizes: [Option<u32>; 2],
) -> Result<(LibEntry, Option<BuildStamp>, Option<Note>), StoreError> {
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

/// The page of `shelf`'s Sounds at `at`, in its order, into `out` in
/// place: one directory pass, then a peek of each seen side of the kept ids.
#[optimize(size)]
pub fn list_library<S: Store>(
    card: &mut Card,
    store: &mut S,
    shelf: Shelf,
    at: PageAt,
    out: &mut LibPage,
) -> CardOut<Result<(), LibNote>> {
    out.len = 0;
    out.prev = false;
    out.next = false;
    out.shelf = shelf;
    out.set_stamp = None;
    out.vol = None;
    let at_rank = match at {
        PageAt::First => None,
        PageAt::After(x) | PageAt::Before(x) => shelf.rank(x),
    };
    // An id off the shelf names no place on it: its first page.
    let (at, at_rank) = at_rank.map_or((PageAt::First, 0), |r| (at, r));
    let run = card.run(store, |s, r| {
        let vol = r.volume();
        out.vol = Some(vol);
        let mut near = Near {
            ids: [(0, LibId::new(1).unwrap(), [None; 2]); MAX_LISTED],
            len: 0,
            high: matches!(at, PageAt::Before(_)),
            more: false,
            back: false,
        };
        match s.list(vol, Dir::Sounds, &mut |f, size| {
            if let Some(id) = lib_id(f)
                && let Some(n) = shelf.rank(id)
            {
                let side = usize::from(f.ext() == b"B");
                near.see(at, at_rank, n, id, side, size);
            }
        }) {
            Ok(()) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        for &(_, id, sizes) in &near.ids[..near.len] {
            let (e, stamp, note) = peek_pair(s, vol, id, sizes)?;
            // The first described row's: a stray file isn't the set's.
            if out.set_stamp.is_none() && note.is_some() {
                out.set_stamp = stamp;
            }
            out.entries[usize::from(out.len)] = Some(e);
            out.len += 1;
        }
        (out.prev, out.next) = match at {
            PageAt::First => (false, near.more),
            PageAt::After(_) => (near.back, near.more),
            PageAt::Before(_) => (near.more, near.back),
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
    #[optimize(size)]
    pub fn readable(&self) -> bool {
        self.on_card && self.held > 0
    }

    /// Why it is stored absent: damaged on the card, or not there.
    #[optimize(size)]
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
    /// Operator kinds past the formant budget, read as W1.
    pub over: bool,
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
    /// The Part or its kit moved since it was confirmed, or the scratch
    /// is no longer the read's.
    Changed,
    /// `None`: every slot taken.
    Full(Option<PoolFull>),
}

impl From<AdoptError> for ReplaceError {
    #[optimize(size)]
    fn from(e: AdoptError) -> Self {
        match e {
            AdoptError::Changed => ReplaceError::Changed,
            AdoptError::Full(f) => ReplaceError::Full(f),
        }
    }
}

/// `source`'s pool need now: `None` when the pool holds it with its bytes.
#[optimize(size)]
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

/// The sizes of `id`'s two sides on `vol`, as the directory has them.
#[optimize(size)]
fn sides_of<S: Store>(s: &mut S, vol: VolumeId, id: LibId) -> Result<[Option<u32>; 2], StoreError> {
    let mut sizes = [None; 2];
    match s.list(vol, Dir::Sounds, &mut |f, size| {
        if lib_id(f) == Some(id) {
            sizes[usize::from(f.ext() == b"B")] = Some(size);
        }
    }) {
        Ok(()) | Err(StoreError::NotFound) => Ok(sizes),
        Err(e) => Err(e),
    }
}

/// `file` still the row listed, on the card in the slot (`vol`): its
/// sides re-listed and re-peeked, refused when the card, or the pair,
/// moved since; with its description. A load and `peek_note` share it.
#[optimize(size)]
fn recheck<S: Store>(
    s: &mut S,
    vol: VolumeId,
    file: LibFile,
) -> Result<Result<Option<Note>, LibNote>, StoreError> {
    if vol != file.vol {
        return Ok(Err(LibNote::Store(StoreError::VolumeChanged(vol))));
    }
    let sizes = sides_of(s, vol, file.id)?;
    let (now, _, note) = peek_pair(s, vol, file.id, sizes)?;
    Ok(if now.seen.generation.is_none() && now.err.is_none() {
        Err(LibNote::Gone)
    } else if now.seen != file.seen {
        Err(LibNote::Changed)
    } else {
        Ok(note)
    })
}

/// The description of `file`, a listed factory row, peeked as the listing
/// peeks it: FACTORY keeps the selected row's alone (showcase spec § 1,
/// Cost). Refused as a load is when the card or the file moved since.
#[optimize(size)]
pub fn peek_note<S: Store>(
    card: &mut Card,
    store: &mut S,
    file: LibFile,
) -> CardOut<Result<Option<Note>, LibNote>> {
    let run = card.run(store, |s, r| Ok(recheck(s, r.volume(), file)?));
    let (out, event) = split(run);
    CardOut {
        out: out.and_then(|r| r),
        event,
    }
}

/// A library Sound file's check, with no target: the Sound's
/// (`SoundCheck`), and a DRUM Part's kit records' through `KitDoor` (ADR
/// 0305). The one checker of every library file that is not being
/// loaded: a save's read-back, the console's `put`, its delete and its
/// pair judge. `SoundCheck` itself refuses a kit record, so only here
/// and in a load's `PartDecoder` is one accepted.
pub struct PartCheck {
    sound: SoundCheck,
    scan: KitScan,
}

impl PartCheck {
    #[optimize(size)]
    pub fn new() -> Self {
        PartCheck {
            sound: SoundCheck::new(),
            scan: KitScan::NEW,
        }
    }
}

impl Default for PartCheck {
    #[optimize(size)]
    fn default() -> Self {
        Self::new()
    }
}

impl Check for PartCheck {
    const KIND: FileKind = <SoundCheck as Check>::KIND;

    #[optimize(size)]
    fn event(&mut self, e: Event<'_>) -> Result<(), FileError> {
        if KitDoor::scan(&mut self.scan, &e, None)? {
            return Ok(());
        }
        self.sound.event(e)
    }

    #[optimize(size)]
    fn end(&mut self) -> Result<(), FileError> {
        KitDoor::found(&self.scan)?;
        self.sound.end()
    }

    #[optimize(size)]
    fn side(&mut self, s: Side) {
        self.sound.side(s)
    }
}

/// A library file as a Part's load reads it: the Sound onto the pool's
/// scratch (`LibraryDecoder`) and a DRUM Sound's kit onto the scratch
/// kit (ADR 0305). Pass 1 checks every kit record and touches nothing;
/// pass 2 writes the scratch, which only `adopt` reads. A kit record is
/// the kit's alone: the Sound's decoder never sees it.
struct PartDecoder<'a> {
    sound: LibraryDecoder<'a>,
    kit: &'a mut Kit,
    scan: KitScan,
    found: ReadKit,
}

impl Check for PartDecoder<'_> {
    const KIND: FileKind = <LibraryDecoder<'_> as Check>::KIND;

    #[optimize(size)]
    fn event(&mut self, e: Event<'_>) -> Result<(), FileError> {
        if KitDoor::scan(&mut self.scan, &e, None)? {
            return Ok(());
        }
        self.sound.event(e)
    }

    #[optimize(size)]
    fn end(&mut self) -> Result<(), FileError> {
        KitDoor::found(&self.scan)?;
        self.sound.end()
    }
}

impl DecodeInPlace for PartDecoder<'_> {
    #[optimize(size)]
    fn apply(&mut self, e: Event<'_>) -> Result<(), FileError> {
        if KitDoor::scan(&mut self.scan, &e, Some(&mut *self.kit))? {
            return Ok(());
        }
        self.sound.apply(e)
    }

    #[optimize(size)]
    fn finish(&mut self) -> Result<(), FileError> {
        self.found = KitDoor::found(&self.scan)?;
        self.sound.finish()
    }
}

/// A library file read into the scratch for one confirmed load: what
/// `adopt` takes, and the only thing it takes. Not `Copy`: one read, one
/// adopt. It holds the confirmation, what the read said, the file's kit
/// and the scratch's epoch, which every scratch writer moves, so a kit
/// or Sound from another file or an earlier read can't land.
#[must_use]
#[derive(Debug)]
pub struct ReadPart {
    go: Confirmed<LibraryKit>,
    read: LibRead,
    kit: ReadKit,
    epoch: u16,
}

impl ReadPart {
    #[optimize(size)]
    pub fn part(&self) -> PartId {
        self.go.target().part()
    }

    #[optimize(size)]
    pub fn file(&self) -> LibFile {
        self.go.target().file()
    }

    #[optimize(size)]
    pub fn read(&self) -> LibRead {
        self.read
    }

    #[optimize(size)]
    pub fn kit(&self) -> ReadKit {
        self.kit
    }

    /// A read that never ran, for a pending load's tests.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn for_test(p: &Project, go: Confirmed<LibraryKit>, read: LibRead) -> ReadPart {
        ReadPart {
            go,
            read,
            kit: ReadKit::Closed,
            epoch: p.pool.scratch_epoch(),
        }
    }
}

/// `go`'s file decoded into the pool's scratch, nothing else touched: its
/// sides re-peeked first, so a pair gone or re-written since the listing
/// refuses. A clobbered scratch is harmless: only `adopt` reads it, and
/// only through this read's `ReadPart`.
#[optimize(size)]
pub fn read_library_sound<S: Store>(
    card: &mut Card,
    store: &mut S,
    p: &mut Project,
    go: Confirmed<LibraryKit>,
) -> CardOut<Result<ReadPart, LibNote>> {
    let file = go.target().file();
    let run = card.run(store, |s, r| {
        let vol = r.volume();
        if let Err(n) = recheck(s, vol, file)? {
            return Ok(Err(n));
        }
        let ab = library_file(file.id);
        let (scratch, kept) = p.pool.scratch_kept();
        let mut d = PartDecoder {
            sound: LibraryDecoder::new(scratch, kept),
            kit: p.kits.scratch_mut(),
            scan: KitScan::NEW,
            found: ReadKit::Closed,
        };
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
        let (mut skips, over, kit) = (d.sound.skips(), d.sound.over_budget(), d.found);
        // A kit value read as another is a skip, as a Sound's is.
        if matches!(kit, ReadKit::Found(f) if f.lossy()) {
            skips = skips.saturating_add(1);
        }
        let source = match p.pool.scratch_ref().source_ref() {
            Some(sr) => source_need(s, vol, p, sr)?,
            None => None,
        };
        Ok(Ok((
            LibRead {
                skips,
                over,
                source,
            },
            kit,
        )))
    });
    let (out, event) = split(run);
    let epoch = p.pool.scratch_epoch();
    CardOut {
        out: out.and_then(|r| r).map(|(read, kit)| ReadPart {
            go,
            read,
            kit,
            epoch,
        }),
        event,
    }
}

impl Project {
    /// What adopting the scratch Sound takes: `Ok(Some(slot))` for a slot
    /// already holding it, `Ok(None)` when it fits. Its slot cost and a
    /// free Sound slot, plus, for a source on the card, its cost and a
    /// free source slot. A source not on the card is stored absent: no
    /// cost, no slot.
    /// Closed, a Sound with a kept map record never dedupes: a slot
    /// equal to it but for the map would lose it (#862).
    #[optimize(size)]
    pub fn library_needs(
        &self,
        src: Option<SourceNeed>,
    ) -> Result<Option<SlotId>, Option<PoolFull>> {
        let dedupe = !self.pool.scratch_record();
        self.needs(self.pool.scratch_ref(), src, dedupe)
    }

    /// `library_needs` for `snd`: the host's demo build places a Sound as
    /// a library load would.
    #[optimize(size)]
    pub fn needs_for(
        &self,
        snd: &Sound,
        src: Option<SourceNeed>,
    ) -> Result<Option<SlotId>, Option<PoolFull>> {
        self.needs(snd, src, true)
    }

    #[optimize(size)]
    fn needs(
        &self,
        snd: &Sound,
        src: Option<SourceNeed>,
        dedupe: bool,
    ) -> Result<Option<SlotId>, Option<PoolFull>> {
        let pool = &self.pool;
        let crc = sound_crc(snd);
        let dedupe = SlotId::ALL
            .into_iter()
            .filter(|_| dedupe)
            .find(|&s| pool.get(s).is_some_and(|e| e.crc() == crc));
        let mut needs = Cost::default();
        if dedupe.is_none() {
            let slot = pool.first_free().ok_or(None)?;
            needs = needs + pool.needs(slot, snd);
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

    /// `r`'s Sound, in the scratch, into the first free slot (or the one
    /// holding it), with `staged`'s source, the Part loaded from it and,
    /// for a DRUM Sound, the Part's kit replaced by the file's: both
    /// confirmations re-checked and the scratch still this read's, then
    /// the budget, before anything moves. A refusal changes nothing.
    /// Only a read adopts, and only once:
    ///
    /// ```compile_fail,E0308
    /// use chimera_engine::project::{LibraryPart, Project, ReplaceGuard, TemplateCrc};
    /// fn f(p: &mut Project, t: TemplateCrc, s: LibraryPart) {
    ///     if let Ok(c) = ReplaceGuard::check(p, t, s) {
    ///         let _ = p.adopt(c, None);
    ///     }
    /// }
    /// ```
    #[optimize(size)]
    pub fn adopt(&mut self, r: ReadPart, staged: Option<Staged>) -> Result<Adopted, AdoptError> {
        if !r.go.still(self) || r.epoch != self.pool.scratch_epoch() {
            return Err(AdoptError::Changed);
        }
        let part = r.go.target().part();
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
                self.pool
                    .store_scratch(s, true)
                    .expect("library_needs: it fits");
                s
            }
        };
        self.load_part(PartSource {
            part,
            from: PartFrom::Slot(slot),
        })
        .expect("a slot just checked or stored decodes");
        match r.kit {
            ReadKit::Found(found) => KitDoor::land(self, part, found),
            // Bank 2 closed at the read: the Part's kit stays.
            ReadKit::Closed => {}
        }
        Ok(Adopted {
            slot,
            deduped: dedupe.is_some(),
        })
    }
}
