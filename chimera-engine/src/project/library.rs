//! The Sound library on the card (library spec § Save): a Part's Sound to
//! `/CHIMERA/SOUNDS/S#######.A`, stamped with the build.

use core::ops::ControlFlow;

use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError, VolumeId};

use super::store::{CardOut, MAX_LISTED, split};
use crate::name::SoundName;
use crate::params::EngineType;
use crate::preset::Sound;
use crate::storage::{
    BuildStamp, Card, CardFault, FileError, Generation, HEADER_LEN, ID_WINDOW, LIB_PEEK, LibId,
    NextId, Peek, SaveError, Side, SideState, SoundCheck, check_file, encode_library_sound, lib_id,
    library_file, next_user_id, peek_library, save_ab,
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
            encode_library_sound(s, stamp, w)
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
/// error that left it unreadable.
#[derive(Clone, Copy, Debug)]
pub struct LibEntry {
    pub file: LibFile,
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
            .position(|e| e.is_some_and(|e| e.file.id == id))
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
fn lib_entry(
    id: LibId,
    vol: VolumeId,
    sides: [Option<(Result<Peek, FileError>, u32)>; 2],
) -> LibEntry {
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
        file: LibFile {
            id,
            vol,
            seen: LibSeen {
                generation: best.map(|(p, _)| p.generation),
                name: best.and_then(|(p, _)| p.name),
            },
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
            let file = library_file(id);
            let mut sides = [None, None];
            for (i, side) in [Side::A, Side::B].into_iter().enumerate() {
                if let Some(size) = sizes[i]
                    && let Some(p) = peek_side(s, vol, file.side(side), size)?
                {
                    sides[i] = Some((p, size));
                }
            }
            out.entries[usize::from(out.len)] = Some(lib_entry(id, vol, sides));
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
