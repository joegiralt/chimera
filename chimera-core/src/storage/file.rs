//! A/B files (ADR 0045): two-pass loads, streamed saves, and the pick rule
//! that keeps a generation loadable through a cut at any block.

use core::ops::ControlFlow;

use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError};

use crate::name::Name;

use super::card::{CardFault, Ready};
use super::frame::{Event, FileError, FileKind, Framer, Generation, Header, Side};
use super::record::{RecordWriter, write_file};

/// Decodes one kind of file, a pass at a time; each pass starts at
/// `Event::Header`, which resets the decoder.
///
/// Pass 1 (`apply` false) only checks. Pass 2 (`apply` true) runs only after
/// pass 1's `finish` and `end` were Ok, and stages what it hears: nothing
/// reaches the target before `end(true)`, which is called only once pass
/// 2's `finish` is Ok and its CRC equals pass 1's. So a card that changes
/// between the passes never leaves the target half-applied.
pub trait Decode {
    const KIND: FileKind;
    fn event(&mut self, e: Event<'_>, apply: bool) -> Result<(), FileError>;
    fn end(&mut self, apply: bool) -> Result<(), FileError>;
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
        let SaveError::Store(e) = *self;
        Some(e)
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

/// The side a save writes, and its generation: never the side `pick`
/// keeps (the one it loads, or refuses as needing newer firmware). With
/// nothing kept, the side holding less: missing before torn before
/// headerless before present, the older of two present sides (a tie
/// writes B, as A is taken for the newer), else A. The generation is the
/// newest present side's + 1, or `FIRST`.
pub fn write_target(a: SideState, b: SideState) -> (Side, Generation) {
    use SideState::{Headerless, Missing, Present, Torn};
    let side = match pick(a, b) {
        Pick::Load(k) | Pick::Refuse(k, NNF) => k.other(),
        _ => {
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
    };
    let generation = |s| match s {
        Present { generation, .. } => Some(generation),
        _ => None,
    };
    let newest = match (generation(a), generation(b)) {
        (Some(x), Some(y)) if y.is_newer_than(x) => Some(y),
        (x, y) => x.or(y),
    };
    (side, newest.map_or(Generation::FIRST, Generation::next))
}

/// The side a load wouldn't keep goes first, so a cut between the two
/// deletes leaves the kept one: "not deleted".
pub fn delete_order(a: SideState, b: SideState) -> [Side; 2] {
    let first = write_target(a, b).0;
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

fn check<S: Store, D: Decode>(
    s: &mut S,
    r: &Ready,
    f: FileName,
    d: &mut D,
) -> Result<Checked, StoreError> {
    let scan = scan(s, r, f, Some(D::KIND), &mut |e| d.event(e, false));
    Checked::of(scan, || d.end(false))
}

/// Pass 1: the side as `d` judges it, the target untouched. A store error
/// other than `NotFound` or `Corrupt` is a card fault and comes back as is.
pub fn check_file<S: Store, D: Decode>(
    s: &mut S,
    r: &Ready,
    f: FileName,
    d: &mut D,
) -> Result<SideState, StoreError> {
    check(s, r, f, d).map(|c| c.state())
}

/// The side's framing only: its kind, header and CRC, and the
/// must-understand bit, which the framer enforces.
pub fn check_frame<S: Store>(
    s: &mut S,
    r: &Ready,
    f: FileName,
    kind: FileKind,
) -> Result<SideState, StoreError> {
    frame_state(s, r, f, Some(kind))
}

fn frame_state<S: Store>(
    s: &mut S,
    r: &Ready,
    f: FileName,
    kind: Option<FileKind>,
) -> Result<SideState, StoreError> {
    let scan = scan(s, r, f, kind, &mut |_| Ok(()));
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
    match scan(s, r, f, Some(D::KIND), &mut |e| d.event(e, true)) {
        Ok(Scan::Passed { header, crc: again }) if again == crc => {
            d.end(true).map_err(LoadError::File)?;
            Ok(header)
        }
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

/// Checks both sides, `pick`s one and applies it.
pub fn load_ab<S: Store, D: Decode>(
    s: &mut S,
    r: &Ready,
    f: AbFile,
    d: &mut D,
) -> Result<Header, LoadError> {
    let a = check(s, r, f.side(Side::A), d)?;
    let b = check(s, r, f.side(Side::B), d)?;
    match pick(a.state(), b.state()) {
        Pick::Load(Side::A) => apply(s, r, f.side(Side::A), d, a),
        Pick::Load(Side::B) => apply(s, r, f.side(Side::B), d, b),
        Pick::Refuse(_, e) => Err(LoadError::File(e)),
        Pick::Missing => Err(LoadError::Missing),
    }
}

/// Streams `header → body → trailer` to the side `write_target` names,
/// judged by framing (a decoder's own verdicts need a target to decode
/// into). Returns the generation written.
pub fn save_ab<S: Store>(
    s: &mut S,
    r: &Ready,
    f: AbFile,
    kind: FileKind,
    name: Option<Name<16>>,
    body: &mut dyn FnMut(&mut RecordWriter<'_>) -> Result<(), StoreError>,
) -> Result<Generation, SaveError> {
    let a = check_frame(s, r, f.side(Side::A), kind)?;
    let b = check_frame(s, r, f.side(Side::B), kind)?;
    let (side, generation) = write_target(a, b);
    let h = Header {
        kind,
        generation,
        name,
    };
    s.write(r.volume(), f.side(side), &mut |sink| {
        write_file(sink, &h, body)
    })?;
    Ok(generation)
}

/// Deletes the sides there are in `delete_order`, judged by framing of any
/// kind.
pub fn delete_ab<S: Store>(s: &mut S, r: &Ready, f: AbFile) -> Result<(), StoreError> {
    let a = frame_state(s, r, f.side(Side::A), None)?;
    let b = frame_state(s, r, f.side(Side::B), None)?;
    for side in delete_order(a, b) {
        let state = if side == Side::A { a } else { b };
        if state != SideState::Missing {
            s.delete(r.volume(), f.side(side))?;
        }
    }
    Ok(())
}
