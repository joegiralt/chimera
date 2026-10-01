//! A/B files (ADR 0045): two-pass loads, streamed saves, and the pick rule
//! that keeps a generation loadable through a cut at any block.

use core::ops::ControlFlow;

use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError};

use crate::name::Name;

use super::card::{CardFault, Ready};
use super::frame::{Event, FileError, FileKind, Framer, Generation, Header, Side};
use super::record::{RecordWriter, write_file};

/// Pass 1 of one kind of file: judges it and touches nothing. A pass
/// starts at `Event::Header`, which resets the checker. `end` runs only
/// once the framer's `finish` is Ok.
///
/// Saves and deletes need only this, so they take a `Check` with no target.
pub trait Check {
    const KIND: FileKind;
    fn event(&mut self, e: Event<'_>) -> Result<(), FileError>;
    fn end(&mut self) -> Result<(), FileError>;
}

/// Pass 2 on top of the check: runs only after pass 1's `finish` and `end`
/// were Ok, and stages what it hears. Nothing reaches the target before
/// `commit`, which is called only once pass 2's `finish` is Ok and its CRC
/// equals pass 1's. So a card that changes between the passes never leaves
/// the target half-applied.
pub trait Decode: Check {
    fn apply(&mut self, e: Event<'_>) -> Result<(), FileError>;
    fn commit(&mut self) -> Result<(), FileError>;
}

/// Pass 2 in place: writes the target as it hears events. Used only where
/// staging can't fit (a project); `load_ab_in_place` says whether the
/// target was touched.
pub trait DecodeInPlace: Check {
    fn apply(&mut self, e: Event<'_>) -> Result<(), FileError>;
    fn finish(&mut self) -> Result<(), FileError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Store(StoreError),
    File(FileError),
    /// Neither side exists.
    Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveError {
    Store(StoreError),
    /// The side a load would keep can't be read by this firmware
    /// (`NeedsNewerFirmware`): a save would be shadowed by it, so none is
    /// written. Deleting the pair overrides it.
    File(FileError),
}

impl From<StoreError> for LoadError {
    fn from(e: StoreError) -> Self {
        LoadError::Store(e)
    }
}

impl From<StoreError> for SaveError {
    fn from(e: StoreError) -> Self {
        SaveError::Store(e)
    }
}

/// A failed `load_ab_in_place`: `clobbered` once pass 2 had started, so
/// the target is part written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InPlaceError {
    pub err: LoadError,
    pub clobbered: bool,
}

/// Before pass 2: the target is untouched.
impl From<StoreError> for InPlaceError {
    fn from(e: StoreError) -> Self {
        InPlaceError {
            err: LoadError::Store(e),
            clobbered: false,
        }
    }
}

impl CardFault for InPlaceError {
    fn store_error(&self) -> Option<StoreError> {
        self.err.store_error()
    }
}

/// A file error is the file's, never the card's.
impl CardFault for LoadError {
    fn store_error(&self) -> Option<StoreError> {
        match *self {
            LoadError::Store(e) => Some(e),
            LoadError::File(_) | LoadError::Missing => None,
        }
    }
}

impl CardFault for SaveError {
    fn store_error(&self) -> Option<StoreError> {
        match *self {
            SaveError::Store(e) => Some(e),
            SaveError::File(_) => None,
        }
    }
}

/// An A/B pair: `<stem>.A` and `<stem>.B` in `dir`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbFile {
    dir: Dir,
    stem: [u8; 8],
    len: u8,
}

impl AbFile {
    pub const SYSTEM: AbFile = AbFile {
        dir: Dir::Chimera,
        stem: *b"SYSTEM\0\0",
        len: 6,
    };

    /// `stem` is 1..=8 bytes of A-Z 0-9.
    pub fn new(dir: Dir, stem: &[u8]) -> Option<AbFile> {
        FileName::new(dir, stem, Side::A.ext())?;
        let mut s = [0; 8];
        s[..stem.len()].copy_from_slice(stem);
        Some(AbFile {
            dir,
            stem: s,
            len: stem.len() as u8,
        })
    }

    pub fn side(&self, s: Side) -> FileName {
        FileName::new(self.dir, &self.stem[..self.len as usize], s.ext())
            .expect("a stem `new` checked, or SYSTEM's")
    }
}

/// One side of a pair, as pass 1 found it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SideState {
    Missing,
    /// What a cut can leave: `Truncated` or `BadCrc`. A chain the store
    /// finds broken (`StoreError::Corrupt`) is `Truncated`.
    Torn(FileError),
    /// The CRC matched but the header isn't one this firmware reads (a newer
    /// format version, a bad magic or name): there is no generation to order
    /// it by.
    Headerless(FileError),
    /// The header was read; `err` is any later verdict.
    Present {
        generation: Generation,
        err: Option<FileError>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Load(Side),
    Refuse(Side, FileError),
    /// Neither side exists.
    Missing,
}

const NNF: FileError = FileError::NeedsNewerFirmware;

/// Whether `b` goes before `a`, newest first: by generation between
/// present sides, a tie to A. A side that needs newer firmware and has no
/// header can't be shown to be older, so it goes first; then any present
/// side.
fn b_first(a: SideState, b: SideState) -> bool {
    use SideState::{Headerless, Present};
    match (a, b) {
        (Present { generation: ga, .. }, Present { generation: gb, .. }) => gb.is_newer_than(ga),
        (Headerless(NNF), _) => false,
        (_, Headerless(NNF)) => true,
        (Present { .. }, _) => false,
        (_, Present { .. }) => true,
        _ => false,
    }
}

/// The side a load takes: the newest that decodes. One that needs newer
/// firmware, met first, is refused: an older file never shadows it. Any
/// other error falls back. With nothing to load, the error is the newest
/// present side's, else a headerless one's, else a torn one's, A first;
/// `Missing` only when neither side exists.
pub fn pick(a: SideState, b: SideState) -> Pick {
    use SideState::{Headerless, Present, Torn};
    let ab = [(Side::A, a), (Side::B, b)];
    let order = if b_first(a, b) { [ab[1], ab[0]] } else { ab };
    for (side, s) in order {
        match s {
            Present { err: None, .. } => return Pick::Load(side),
            Present { err: Some(NNF), .. } | Headerless(NNF) => return Pick::Refuse(side, NNF),
            _ => {}
        }
    }
    let present = order.iter().find_map(|&(side, s)| match s {
        Present { err: Some(e), .. } => Some(Pick::Refuse(side, e)),
        _ => None,
    });
    let first = |want: fn(SideState) -> Option<FileError>| {
        ab.iter()
            .find_map(|&(side, s)| want(s).map(|e| Pick::Refuse(side, e)))
    };
    present
        .or_else(|| {
            first(|s| match s {
                Headerless(e) => Some(e),
                _ => None,
            })
        })
        .or_else(|| {
            first(|s| match s {
                Torn(e) => Some(e),
                _ => None,
            })
        })
        .unwrap_or(Pick::Missing)
}

/// The side `pick` doesn't keep (the one it loads, or refuses as needing
/// newer firmware). With nothing kept, the side holding less: missing
/// before torn before headerless before present, the older of two present
/// sides (a tie gives B, as A is taken for the newer), else A.
fn unkept(a: SideState, b: SideState) -> Side {
    use SideState::{Headerless, Missing, Present, Torn};
    match pick(a, b) {
        Pick::Load(k) | Pick::Refuse(k, NNF) => k.other(),
        Pick::Refuse(..) | Pick::Missing => {
            let rank = |s| match s {
                Missing => 0,
                Torn(_) => 1,
                Headerless(_) => 2,
                Present { .. } => 3,
            };
            match (a, b) {
                (Present { .. }, Present { .. }) if b_first(a, b) => Side::A,
                (Present { .. }, Present { .. }) => Side::B,
                _ if rank(b) < rank(a) => Side::B,
                _ => Side::A,
            }
        }
    }
}

/// The side a save writes, `unkept`, and its generation: the newest present
/// side's + 1, or `FIRST`.
///
/// `NeedsNewerFirmware` when `pick` refuses a side as needing newer
/// firmware, whether its header was read or not: a load would never get
/// past that side to what the save wrote, so nothing is written. Never
/// shadow, never write.
pub fn write_target(a: SideState, b: SideState) -> Result<(Side, Generation), FileError> {
    if let Pick::Refuse(_, NNF) = pick(a, b) {
        return Err(NNF);
    }
    let generation = |s| match s {
        SideState::Present { generation, .. } => Some(generation),
        _ => None,
    };
    let newest = match (generation(a), generation(b)) {
        (Some(x), Some(y)) if y.is_newer_than(x) => Some(y),
        (x, y) => x.or(y),
    };
    Ok((
        unkept(a, b),
        newest.map_or(Generation::FIRST, Generation::next),
    ))
}

/// The side a load wouldn't keep goes first, so a cut between the two
/// deletes leaves the kept one: "not deleted". A pair a save refuses
/// deletes too: that is how the user overrides a newer file.
pub fn delete_order(a: SideState, b: SideState) -> [Side; 2] {
    let first = unkept(a, b);
    [first, first.other()]
}

/// Feeds a `Store::read` through a `Framer`.
struct Feed<'a> {
    framer: Option<Framer>,
    err: Option<FileError>,
    on: &'a mut dyn FnMut(Event<'_>) -> Result<(), FileError>,
}

impl ReadSink for Feed<'_> {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        match Framer::new(len) {
            Ok(f) => {
                self.framer = Some(f);
                ControlFlow::Continue(())
            }
            Err(e) => {
                self.err = Some(e);
                ControlFlow::Break(())
            }
        }
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        let Some(f) = self.framer.as_mut() else {
            return ControlFlow::Break(());
        };
        match f.push(bytes, self.on) {
            Ok(()) => ControlFlow::Continue(()),
            Err(e) => {
                self.err = Some(e);
                ControlFlow::Break(())
            }
        }
    }
}

/// One read of a file through the framer.
enum Scan {
    /// The CRC matched and every verdict so far was Ok.
    Passed { header: Header, crc: u32 },
    Failed {
        header: Option<Header>,
        err: FileError,
    },
}

/// Reads `f` through a `Framer`, handing `on` its events. A `kind` other
/// than the file's is `WrongKind`, deferred like every verdict; `None`
/// takes any kind.
fn scan<S: Store>(
    s: &mut S,
    r: &Ready,
    f: FileName,
    kind: Option<FileKind>,
    on: &mut dyn FnMut(Event<'_>) -> Result<(), FileError>,
) -> Result<Scan, StoreError> {
    let mut header = None;
    let mut on = |e: Event<'_>| {
        if let Event::Header(h) = e {
            header = Some(h);
            if kind.is_some_and(|k| k != h.kind) {
                return Err(FileError::WrongKind);
            }
        }
        on(e)
    };
    let mut feed = Feed {
        framer: None,
        err: None,
        on: &mut on,
    };
    s.read(r.volume(), f, &mut feed)?;
    let Feed { framer, err, .. } = feed;
    let verdict = match (err, &framer) {
        (Some(e), _) => Err(e),
        (None, Some(fr)) => fr.finish().map(|()| fr.crc()),
        // A store that never called `begin` gave no length.
        (None, None) => Err(FileError::Truncated),
    };
    Ok(match (verdict, header) {
        (Ok(crc), Some(header)) => Scan::Passed { header, crc },
        (Err(err), header) => Scan::Failed { header, err },
        // `finish` is Ok only past a header the callback took: unreachable,
        // and read as damage rather than trusted.
        (Ok(_), None) => Scan::Failed {
            header: None,
            err: FileError::Corrupt,
        },
    })
}

/// Pass 1's verdict on a side. One that passed keeps the CRC pass 2 must
/// see again.
enum Checked {
    Missing,
    Passed {
        header: Header,
        crc: u32,
    },
    Failed {
        generation: Option<Generation>,
        err: FileError,
    },
}

impl Checked {
    /// `end` runs only on a side whose framing passed.
    fn of(
        scan: Result<Scan, StoreError>,
        end: impl FnOnce() -> Result<(), FileError>,
    ) -> Result<Checked, StoreError> {
        let (header, err) = match scan {
            Err(StoreError::NotFound) => return Ok(Checked::Missing),
            // A broken or short chain, which a cut elsewhere can leave.
            Err(StoreError::Corrupt) => (None, FileError::Truncated),
            Err(e) => return Err(e),
            Ok(Scan::Passed { header, crc }) => match end() {
                Ok(()) => return Ok(Checked::Passed { header, crc }),
                Err(e) => (Some(header), e),
            },
            Ok(Scan::Failed { header, err }) => (header, err),
        };
        Ok(Checked::Failed {
            generation: header.map(|h| h.generation),
            err,
        })
    }

    fn state(&self) -> SideState {
        match *self {
            Checked::Missing => SideState::Missing,
            Checked::Passed { header, .. } => SideState::Present {
                generation: header.generation,
                err: None,
            },
            Checked::Failed { err, .. } if err.is_torn() => SideState::Torn(err),
            Checked::Failed {
                generation: Some(generation),
                err,
            } => SideState::Present {
                generation,
                err: Some(err),
            },
            Checked::Failed {
                generation: None,
                err,
            } => SideState::Headerless(err),
        }
    }
}

fn check<S: Store, C: Check>(
    s: &mut S,
    r: &Ready,
    f: FileName,
    c: &mut C,
) -> Result<Checked, StoreError> {
    let scan = scan(s, r, f, Some(C::KIND), &mut |e| c.event(e));
    Checked::of(scan, || c.end())
}

/// Pass 1: the side as `c` judges it. A store error other than `NotFound`
/// or `Corrupt` is a card fault and comes back as is.
pub fn check_file<S: Store, C: Check>(
    s: &mut S,
    r: &Ready,
    f: FileName,
    c: &mut C,
) -> Result<SideState, StoreError> {
    check(s, r, f, c).map(|c| c.state())
}

/// The side's framing only: its kind, header and CRC, and the
/// must-understand bit, which the framer enforces. This is not how a save
/// or a delete judges a side: they run the load's own `Check` (`check_file`),
/// so a side a load rejects is never kept over the one it loads.
#[doc(hidden)]
pub fn check_frame<S: Store>(
    s: &mut S,
    r: &Ready,
    f: FileName,
    kind: FileKind,
) -> Result<SideState, StoreError> {
    let scan = scan(s, r, f, Some(kind), &mut |_| Ok(()));
    Checked::of(scan, || Ok(())).map(|c| c.state())
}

/// Pass 2 on a side pass 1 judged: it applies only if it reads the bytes
/// pass 1 passed. Other bytes (the card changed between the passes) are
/// `BadCrc`, and the target is untouched.
fn apply<S: Store, D: Decode>(
    s: &mut S,
    r: &Ready,
    f: FileName,
    d: &mut D,
    checked: Checked,
) -> Result<Header, LoadError> {
    let crc = match checked {
        Checked::Passed { crc, .. } => crc,
        Checked::Missing => return Err(LoadError::Missing),
        Checked::Failed { err, .. } => return Err(LoadError::File(err)),
    };
    let header = pass_two(s, r, f, D::KIND, crc, &mut |e| d.apply(e))?;
    d.commit().map_err(LoadError::File)?;
    Ok(header)
}

/// Pass 2's read, for the staged and the in-place loader alike: Ok only
/// if it reads the bytes pass 1 passed (`crc`). Other bytes, or a side
/// gone or broken since, are `BadCrc`; any other store error is the card's.
fn pass_two<S: Store>(
    s: &mut S,
    r: &Ready,
    f: FileName,
    kind: FileKind,
    crc: u32,
    on: &mut dyn FnMut(Event<'_>) -> Result<(), FileError>,
) -> Result<Header, LoadError> {
    match scan(s, r, f, Some(kind), on) {
        Ok(Scan::Passed { header, crc: again }) if again == crc => Ok(header),
        Ok(_) | Err(StoreError::NotFound | StoreError::Corrupt) => {
            Err(LoadError::File(FileError::BadCrc))
        }
        Err(e) => Err(LoadError::Store(e)),
    }
}

/// Checks `f`, then applies it.
pub fn load_file<S: Store, D: Decode>(
    s: &mut S,
    r: &Ready,
    f: FileName,
    d: &mut D,
) -> Result<Header, LoadError> {
    let checked = check(s, r, f, d)?;
    apply(s, r, f, d, checked)
}

/// Pass 1 on both sides: the one judgement `load_ab`, `save_ab` and
/// `delete_ab` share.
fn sides<S: Store, C: Check>(
    s: &mut S,
    r: &Ready,
    f: AbFile,
    c: &mut C,
) -> Result<[Checked; 2], StoreError> {
    Ok([
        check(s, r, f.side(Side::A), c)?,
        check(s, r, f.side(Side::B), c)?,
    ])
}

/// Checks both sides, `pick`s one and applies it.
pub fn load_ab<S: Store, D: Decode>(
    s: &mut S,
    r: &Ready,
    f: AbFile,
    d: &mut D,
) -> Result<Header, LoadError> {
    let [a, b] = sides(s, r, f, d)?;
    match pick(a.state(), b.state()) {
        Pick::Load(Side::A) => apply(s, r, f.side(Side::A), d, a),
        Pick::Load(Side::B) => apply(s, r, f.side(Side::B), d, b),
        Pick::Refuse(_, e) => Err(LoadError::File(e)),
        Pick::Missing => Err(LoadError::Missing),
    }
}

/// `load_ab` with pass 2 in place (ADR 0046): pass 1 and the pick as
/// `load_ab`, so a failure there leaves the target untouched. Pass 2 then
/// writes the target as it reads; any failure from there, or a CRC other
/// than pass 1's, is `clobbered`.
pub fn load_ab_in_place<S: Store, D: DecodeInPlace>(
    s: &mut S,
    r: &Ready,
    f: AbFile,
    d: &mut D,
) -> Result<Header, InPlaceError> {
    let untouched = |err| InPlaceError {
        err,
        clobbered: false,
    };
    let clobbered = |err| InPlaceError {
        err,
        clobbered: true,
    };
    let [a, b] = sides(s, r, f, d)?;
    let (side, checked) = match pick(a.state(), b.state()) {
        Pick::Load(Side::A) => (Side::A, a),
        Pick::Load(Side::B) => (Side::B, b),
        Pick::Refuse(_, e) => return Err(untouched(LoadError::File(e))),
        Pick::Missing => return Err(untouched(LoadError::Missing)),
    };
    let crc = match checked {
        Checked::Passed { crc, .. } => crc,
        Checked::Missing => return Err(untouched(LoadError::Missing)),
        Checked::Failed { err, .. } => return Err(untouched(LoadError::File(err))),
    };
    let header =
        pass_two(s, r, f.side(side), D::KIND, crc, &mut |e| d.apply(e)).map_err(clobbered)?;
    d.finish().map_err(|e| clobbered(LoadError::File(e)))?;
    Ok(header)
}

/// Streams `header → body → trailer` to the side `write_target` names.
/// The sides are judged by `c`, pass 1 as `load_ab` runs it, so a save
/// never writes the side a load would take. Returns the generation written;
/// `File(NeedsNewerFirmware)`, with nothing written, when a load would
/// refuse the pair as needing newer firmware.
pub fn save_ab<S: Store, C: Check>(
    s: &mut S,
    r: &Ready,
    f: AbFile,
    c: &mut C,
    name: Option<Name<16>>,
    body: &mut dyn FnMut(&mut RecordWriter<'_>) -> Result<(), StoreError>,
) -> Result<Generation, SaveError> {
    let [a, b] = sides(s, r, f, c)?;
    let (side, generation) = write_target(a.state(), b.state()).map_err(SaveError::File)?;
    let h = Header {
        kind: C::KIND,
        generation,
        name,
    };
    s.write(r.volume(), f.side(side), &mut |sink| {
        write_file(sink, &h, body)
    })?;
    Ok(generation)
}

/// Deletes the sides there are in `delete_order`, judged by `c` as
/// `load_ab` judges them.
pub fn delete_ab<S: Store, C: Check>(
    s: &mut S,
    r: &Ready,
    f: AbFile,
    c: &mut C,
) -> Result<(), StoreError> {
    let [a, b] = sides(s, r, f, c)?.map(|c| c.state());
    for side in delete_order(a, b) {
        let state = if side == Side::A { a } else { b };
        if state != SideState::Missing {
            s.delete(r.volume(), f.side(side))?;
        }
    }
    Ok(())
}
