//! `ls` and `put`: files onto a card that can't leave the unit.
//! `put` takes a whole project file and saves it the way the unit saves
//! (ADR 0045): to the side a load doesn't keep, with the next generation
//! and its own CRC, so the old side stays loadable until the new one is
//! whole. A side a failed `put` wrote is deleted, so the list shows the
//! pair as it was. Every card operation runs under what is left of
//! `PUT_DEADLINE_MS`.
//!
//! A library Sound goes the same way, its own kind's check in place of
//! the project's; an id already on the card refuses unless `--over`.
//!
//! With `source-writer`, `put` also takes a TAPE source (ADR 0106): in
//! 256 KiB chunks, each checked and under its own deadline, written once,
//! read back and sealed. A file already sealed answers `OK PRESENT`.
//!
//! The bench's check table goes to `/CHIMERA/BENCH/BEDS.CHK`, a whole file
//! of at most 9 KiB, the old one replaced.
//!
//! A tuning goes to `/CHIMERA/TUNINGS/<stem>.SCL` or `.KBM` (ADR 0240,
//! 0259), a whole file of at most 4 KiB, the old one replaced; a scale
//! drops its old map. The Parts on Scala files read theirs again.
//!
//! A controller map goes to `/CHIMERA/MAPS/<stem>.A` or `.B` (ADR 0226,
//! 0255), a whole file of at most 4 KiB: to the side a load does not take,
//! whichever the host names, read back as a map whose name makes the stem,
//! then the live side deleted, so the map put is the one that loads. A put
//! that fails leaves the live side as it was. The UI reads its layers
//! again.
//!
//! Bank 2's image goes to `/CHIMERA/SYSTEM/BANK2.BK2`, the old one deleted
//! first, then read back and judged against this build's seal (ADR 0137):
//! one sealed for another build stays (`OK staged: needs firmware <v>`),
//! anything else is deleted.

use core::fmt::{self, Write};
use core::ops::ControlFlow;

use chimera_hal::Ms;
use chimera_hal::store::{CHUNK, Dir, FileName, Files, ReadSink, StoreError};

use super::{Arg, CardImage, Out, STAGED, Stalled, Words, sound_file::exists};
use crate::bank2::{Seal, Version};
use crate::project::{ProjectCheck, peek_entry, project_file};
use crate::storage::{
    AbFile, Card, CardFault, Check, Crc32, Event, FileError, FileKind, Framer, Generation,
    HEADER_LEN, Header, LibId, Pick, ProjectId, Ready, SaveError, Side, SideState, SoundCheck,
    check_file, check_frame, library_file, peek_header, pick, save_target, write_file,
    write_target,
};
#[cfg(feature = "source-writer")]
use crate::storage::{
    PutRefusal, SourceCheck, Stem, Wrote, source_name, source_target, verify_and_seal,
};
use crate::stream::bench::MAX_BED_CHUNKS;
use crate::stream::layout::FRAMED_CHUNK_BYTES;

/// The largest file `put` takes: a project is about 130 KB.
pub const PUT_MAX_BYTES: u32 = 1 << 20;
/// The whole `put`, from its line to its answer, card work included: a
/// 1 MB bank-2 image at 16 KiB/s is 64 s.
pub const PUT_DEADLINE_MS: u32 = 120_000;
/// Each 512-byte block of data must arrive within this of the last.
pub const PUT_STALL_MS: u32 = 500;
/// Base64 characters a data line holds at most: 48 bytes.
pub const PUT_LINE: usize = 64;
/// The line that ends a transfer early.
pub const ABORT: &[u8] = b"abort";
/// What the unit says once it will take the data.
pub const READY: &str = "READY\n";
/// A source's data comes in chunks of this many bytes (the last may be
/// shorter), each ended by `end <crc32>` and answered `ACK <bytes so far>`.
#[cfg(feature = "source-writer")]
pub const PUT_CHUNK_BYTES: u32 = 256 * 1024;
/// The word that asks to resume a source: reserved for TRON, refused.
pub const RESUME: &[u8] = b"resume";
/// The word that lets a Sound `put` replace an id on the card.
pub const OVER: &[u8] = b"--over";
/// Kept back from the write for judging the pair, or deleting the side,
/// after it.
const VERIFY_MS: u32 = 3_000;
/// The data stops this long before the write's own cap, so the write ends
/// as `put` decides, its chain linked or freed, never cut by the card's.
const SD_MARGIN_MS: u32 = 1_000;
/// After a failure the rest of the host's data is read and dropped until it
/// goes quiet this long, or for `DRAIN_MS` at most, before the `ERR`.
const QUIET_MS: u32 = 100;
const DRAIN_MS: u32 = 5_000;
/// A header and a trailer: the smallest Chimera file.
const MIN_FILE: u32 = 32;
const DECODED: usize = PUT_LINE / 4 * 3;
const PROJECTS: &[u8] = b"/CHIMERA/PROJECTS/";
pub(super) const SOUNDS: &[u8] = b"/CHIMERA/SOUNDS/";
#[cfg(feature = "source-writer")]
const TAPES: &[u8] = b"/CHIMERA/TAPES/";
const BANK2: &[u8] = b"/CHIMERA/SYSTEM/BANK2.BK2";
const BEDS: &[u8] = b"/CHIMERA/BENCH/BEDS.CHK";
const TUNINGS: &[u8] = b"/CHIMERA/TUNINGS/";
pub(super) const MAPS: &[u8] = b"/CHIMERA/MAPS/";
/// A tuning file: what the UI reads (`ui::file_buf::FILE_MAX`).
pub const TUNING_MAX_BYTES: u32 = crate::ui::file_buf::FILE_MAX as u32;
/// The bench's check table: 8 beds of `MAX_BED_CHUNKS` CRCs fit.
pub const BEDS_MAX_BYTES: u32 = 9 * 1024;
const BEDS_FILE: FileName = match FileName::new(Dir::Bench, b"BEDS", b"CHK") {
    Some(f) => f,
    None => panic!("an 8.3 name"),
};
// A bed `put` takes is a source under `PUT_MAX_BYTES`: its table has room.
const _: () = assert!(PUT_MAX_BYTES / FRAMED_CHUNK_BYTES.get() < MAX_BED_CHUNKS as u32);
const _: () = assert!(5 + 8 * (6 + 4 * MAX_BED_CHUNKS) <= BEDS_MAX_BYTES as usize);
#[cfg(feature = "source-writer")]
const _: () = assert!(PUT_CHUNK_BYTES.is_multiple_of(CHUNK as u32));
// The sweep's cap counts on it (ADR 0094).
const _: () = assert!(PUT_MAX_BYTES <= chimera_hal::store::MAX_CHAIN_BYTES);

const _: () = assert!(PUT_LINE.is_multiple_of(4));
const _: () = assert!(PUT_LINE <= super::MAX_LINE);

/// Digits only, at most `max` of them, in `radix`. Out of line: inlined
/// at each of its four calls it doubled `PutArg::parse`.
#[inline(never)]
pub(super) fn number(w: &[u8], radix: u32, max: usize) -> Option<u32> {
    if w.is_empty() || w.len() > max {
        return None;
    }
    w.iter().try_fold(0u32, |n, &c| {
        let d = char::from(c).to_digit(radix)?;
        n.checked_mul(radix)?.checked_add(d)
    })
}

/// `<dir><letter>#######`, any case: the number. Nothing else parses: no
/// other directory, no `..`, no `//`.
pub(super) fn numbered(p: &[u8], dir: &[u8], letter: u8) -> Option<u32> {
    let (d, name) = p.split_at_checked(dir.len())?;
    let (&first, digits) = name.split_first()?;
    if !d.eq_ignore_ascii_case(dir) || !first.eq_ignore_ascii_case(&letter) || digits.len() != 7 {
        return None;
    }
    number(digits, 10, 7)
}

/// `.A` or `.B`, any case: the letter names the pair; the unit picks the
/// side it writes.
fn pair_name(p: &[u8]) -> Option<&[u8]> {
    match p {
        [stem @ .., b'.', b'A' | b'B' | b'a' | b'b'] => Some(stem),
        _ => None,
    }
}

/// `/CHIMERA/PROJECTS/P0000014.A` (or `.B`).
fn project(p: &[u8]) -> Option<ProjectId> {
    ProjectId::new(numbered(pair_name(p)?, PROJECTS, b'P')?)
}

/// `/CHIMERA/SOUNDS/S0000012.A` (or `.B`): user and factory ids alike.
fn sound(p: &[u8]) -> Option<LibId> {
    LibId::new(numbered(pair_name(p)?, SOUNDS, b'S')?)
}

/// `/CHIMERA/TAPES/0BADF00D.SRC`, any case: the stem is the source's CRC.
#[cfg(feature = "source-writer")]
fn source(p: &[u8]) -> Option<Stem> {
    let (dir, name) = p.split_at_checked(TAPES.len())?;
    let (stem, ext) = name.split_at_checked(8)?;
    if !dir.eq_ignore_ascii_case(TAPES) || !ext.eq_ignore_ascii_case(b".SRC") {
        return None;
    }
    number(stem, 16, 8).map(Stem::new)
}

/// Where a `put` goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutTo {
    /// A project's pair, saved the unit's way.
    Project(ProjectId),
    /// A library Sound's pair, the same way. `over`: an id on the card is
    /// replaced, not refused.
    Sound { id: LibId, over: bool },
    /// A TAPE source, by its stem: written once and sealed.
    #[cfg(feature = "source-writer")]
    Source(Stem),
    /// Bank 2's image, staged for `bank2 install` and the boot.
    Bank2,
    /// The bench's check table, replaced whole.
    Beds,
    /// A tuning's `.SCL` or `.KBM`, replaced whole.
    Tuning(FileName),
    /// A controller map's pair, replaced by the file.
    Map(AbFile),
}

/// `/CHIMERA/MAPS/FADE3C04`, any case: the pair.
pub(super) fn map_pair(p: &[u8]) -> Option<AbFile> {
    let (dir, stem) = p.split_at_checked(MAPS.len())?;
    let mut up = [0u8; 8];
    let up = up.get_mut(..stem.len())?;
    up.copy_from_slice(stem);
    up.make_ascii_uppercase();
    dir.eq_ignore_ascii_case(MAPS)
        .then(|| AbFile::new(Dir::Maps, up))
        .flatten()
}

/// `/CHIMERA/TUNINGS/<stem>.SCL` or `.KBM`, any 8.3 stem, any case (ADR
/// 0259).
pub(super) fn tuning(p: &[u8]) -> Option<FileName> {
    let (dir, name) = p.split_at_checked(TUNINGS.len())?;
    if !dir.eq_ignore_ascii_case(TUNINGS) {
        return None;
    }
    let mut up = [0u8; 12];
    let up = up.get_mut(..name.len())?;
    up.copy_from_slice(name);
    up.make_ascii_uppercase();
    match up.split_at_checked(up.len().checked_sub(4)?)? {
        (stem, b".SCL") => FileName::new(Dir::Tunings, stem, b"SCL"),
        (stem, b".KBM") => FileName::new(Dir::Tunings, stem, b"KBM"),
        _ => None,
    }
}

fn put_to(p: &[u8]) -> Option<PutTo> {
    // `put` names files itself: plain 8.3, never a computer's `~` short
    // name for a long one (`rm` and `get` take those, ADR 0259).
    if p.contains(&b'~') {
        return None;
    }
    if p.eq_ignore_ascii_case(BANK2) {
        return Some(PutTo::Bank2);
    }
    if p.eq_ignore_ascii_case(BEDS) {
        return Some(PutTo::Beds);
    }
    if let Some(f) = tuning(p) {
        return Some(PutTo::Tuning(f));
    }
    if let Some(f) = pair_name(p).and_then(map_pair) {
        return Some(PutTo::Map(f));
    }
    let to = project(p)
        .map(PutTo::Project)
        .or_else(|| sound(p).map(|id| PutTo::Sound { id, over: false }));
    #[cfg(feature = "source-writer")]
    let to = to.or_else(|| source(p).map(PutTo::Source));
    to
}

/// `put`'s argument: where, how many bytes and their CRC-32/ISO-HDLC, and
/// whether it asked to resume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PutArg {
    pub to: PutTo,
    pub size: u32,
    pub crc: u32,
    pub resume: bool,
}

impl Arg for PutArg {
    #[cfg(not(feature = "source-writer"))]
    const USAGE: &'static str = "/CHIMERA/PROJECTS/P#######.A, /CHIMERA/SOUNDS/S#######.A \
        [--over], /CHIMERA/BENCH/BEDS.CHK, /CHIMERA/TUNINGS/<stem>.SCL|KBM, /CHIMERA/MAPS/<stem>.A, or /CHIMERA/SYSTEM/BANK2.BK2, its size and CRC-32 \
        in hex";
    #[cfg(feature = "source-writer")]
    const USAGE: &'static str = "/CHIMERA/PROJECTS/P#######.A, /CHIMERA/SOUNDS/S#######.A \
        [--over], /CHIMERA/TAPES/<CRC>.SRC, /CHIMERA/BENCH/BEDS.CHK, /CHIMERA/TUNINGS/<stem>.SCL|KBM, /CHIMERA/MAPS/<stem>.A, or /CHIMERA/SYSTEM/BANK2.BK2, \
        its size and CRC-32 in hex";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let mut to = put_to(words.next()?)?;
        let size = number(words.next()?, 10, 10)?;
        if (to == PutTo::Bank2 && size > chimera_image::BANK2_LEN)
            || (to == PutTo::Beds && size > BEDS_MAX_BYTES)
            || (matches!(to, PutTo::Tuning(_) | PutTo::Map(_)) && size > TUNING_MAX_BYTES)
        {
            return None;
        }
        let crc = number(words.next()?, 16, 8)?;
        let resume = match (words.next(), &mut to) {
            (None, _) => false,
            (Some(w), _) if w.eq_ignore_ascii_case(RESUME) => true,
            (Some(w), PutTo::Sound { over, .. }) if w.eq_ignore_ascii_case(OVER) => {
                *over = true;
                false
            }
            (Some(_), _) => return None,
        };
        words.next().is_none().then_some(PutArg {
            to,
            size,
            crc,
            resume,
        })
    }
}

/// `ls`'s argument: PROJECTS, `tapes`, `sounds` or `maps`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LsArg {
    Projects,
    Tapes,
    Sounds,
    Maps,
}

impl Arg for LsArg {
    const USAGE: &'static str = "tapes, sounds, maps or nothing";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let got = match words.next() {
            None => LsArg::Projects,
            Some(w) if w.eq_ignore_ascii_case(b"tapes") => LsArg::Tapes,
            Some(w) if w.eq_ignore_ascii_case(b"sounds") => LsArg::Sounds,
            Some(w) if w.eq_ignore_ascii_case(b"maps") => LsArg::Maps,
            Some(_) => return None,
        };
        words.next().is_none().then_some(got)
    }
}

/// Why a `put` failed: `ERR put: <message>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutFail {
    ProjectOpen,
    /// A Sound id with a side on the card, and no `--over`.
    Exists,
    TooBig,
    Timeout,
    /// A block's worth of data didn't come within `PUT_STALL_MS`: a short
    /// transfer ends here.
    Stalled,
    Aborted,
    BadData,
    TooLong,
    /// The bytes aren't what the host sent: its CRC.
    BadCrc,
    /// The file itself: its header, its own CRC, or the kind's check after
    /// the write, when the old side still loads.
    File(FileError),
    Store(StoreError),
    /// `resume`: TRON's, not yet.
    NoResume,
    /// A sealed source that fails its check: never rewritten.
    #[cfg(feature = "source-writer")]
    Damaged,
    /// A chunk's `end` CRC isn't its bytes', or a line runs past it.
    #[cfg(feature = "source-writer")]
    Chunk,
    /// The source's own check, at the bytes that show it.
    #[cfg(feature = "source-writer")]
    Source(PutRefusal),
    /// No seal in this build to judge a bank-2 image against.
    Unsealed,
    /// The staged file, read back, is neither good nor stale: deleted.
    NotBank2,
}

impl PutFail {
    pub fn message(self) -> &'static str {
        match self {
            PutFail::ProjectOpen => "project is open",
            PutFail::Exists => "exists",
            PutFail::TooBig => "over 1 MiB",
            PutFail::Timeout => "timeout",
            PutFail::Stalled => "no block for 500 ms",
            PutFail::Aborted => "aborted",
            PutFail::BadData => "bad data line",
            PutFail::TooLong => "more data than its size",
            PutFail::BadCrc => "CRC mismatch",
            PutFail::File(e) => e.message(),
            PutFail::Store(e) => e.message(),
            PutFail::NoResume => "no resume",
            #[cfg(feature = "source-writer")]
            PutFail::Damaged => "damaged",
            #[cfg(feature = "source-writer")]
            PutFail::Chunk => "chunk",
            #[cfg(feature = "source-writer")]
            PutFail::Source(r) => r.message(),
            PutFail::Unsealed => "firmware unsealed",
            PutFail::NotBank2 => "not a bank-2 image",
        }
    }
}

impl From<SaveError> for PutFail {
    fn from(e: SaveError) -> Self {
        match e {
            SaveError::Store(e) => PutFail::Store(e),
            SaveError::File(e) => PutFail::File(e),
            #[cfg(feature = "source-writer")]
            SaveError::Damaged => PutFail::Damaged,
            #[cfg(not(feature = "source-writer"))]
            SaveError::Damaged => PutFail::File(FileError::Corrupt),
        }
    }
}

impl From<StoreError> for PutFail {
    fn from(e: StoreError) -> Self {
        PutFail::Store(e)
    }
}

impl CardFault for PutFail {
    fn store_error(&self) -> Option<StoreError> {
        match *self {
            PutFail::Store(e) => Some(e),
            _ => None,
        }
    }
}

/// The one deadline, on the host link's clock.
#[derive(Clone, Copy)]
struct Due(Ms);

impl Due {
    fn left_ms(self, port: &impl Out) -> u32 {
        PUT_DEADLINE_MS.saturating_sub(port.now().since(self.0))
    }

    /// Caps the card operations from now at what is left less `reserve_ms`:
    /// `Timeout` if nothing is.
    fn arm<F: Files>(self, files: &mut F, port: &impl Out, reserve_ms: u32) -> Result<(), PutFail> {
        match self.left_ms(port).saturating_sub(reserve_ms) {
            0 => Err(PutFail::Timeout),
            left => {
                files.cap(Some(left));
                Ok(())
            }
        }
    }
}

/// How a `put` ended well.
enum Done {
    Saved,
    /// The source was on the card, sealed and whole: no data taken.
    #[cfg(feature = "source-writer")]
    Present,
    /// A bank-2 image sealed for another build: kept for its firmware.
    Stale(Version),
}

/// `put`'s whole answer: `READY` once its directory is made, then `OK`, or
/// `ERR put: <why>` with the pair loading as before; for a source already
/// on the card, `OK PRESENT` in place of `READY`.
/// `open`: the project the UI holds, which `put` never writes under it;
/// `seal`: this build's, which a bank-2 image is judged against.
/// Out of line, like `save`: 460 B smaller under fat LTO, measured.
#[inline(never)]
pub fn put<F: Files, P: Out>(
    arg: PutArg,
    open: Option<ProjectId>,
    seal: Option<Seal>,
    files: &mut F,
    port: &mut P,
) -> Result<(), Stalled> {
    let due = Due(port.now());
    let r = transfer(arg, open, seal, files, port, due);
    files.cap(None);
    port.fresh();
    let mut t = super::Text(port);
    match r {
        Ok(Done::Saved) => t.write_str("OK\n"),
        #[cfg(feature = "source-writer")]
        Ok(Done::Present) => t.write_str("OK PRESENT\n"),
        Ok(Done::Stale(v)) => writeln!(t, "OK staged: needs firmware {}", v.as_str()),
        Err(e) => writeln!(t, "ERR put: {}", e.message()),
    }
    .map_err(|fmt::Error| Stalled)
}

fn transfer<F: Files, P: Out>(
    arg: PutArg,
    open: Option<ProjectId>,
    seal: Option<Seal>,
    files: &mut F,
    port: &mut P,
    due: Due,
) -> Result<Done, PutFail> {
    if arg.resume {
        return Err(PutFail::NoResume);
    }
    if arg.size > PUT_MAX_BYTES {
        return Err(PutFail::TooBig);
    }
    // A Scala file is text, a map a few lines: no Chimera header.
    if arg.size < MIN_FILE && !matches!(arg.to, PutTo::Tuning(_)) {
        return Err(PutFail::File(FileError::Truncated));
    }
    match arg.to {
        PutTo::Project(id) => {
            if open == Some(id) {
                return Err(PutFail::ProjectOpen);
            }
            due.arm(files, port, 0)?;
            Card::new()
                .run(files, |s, r| {
                    session(s, r, Dir::Projects, port, due, |s, port| {
                        let mut src = Incoming::new(arg, port, due);
                        save::<_, _, ProjectCheck>(s, r, project_file(id), &mut src, |s, g| {
                            judge(s, r, id, g)
                        })
                    })
                })
                .and_then(|o| o.result)
                .map(|()| Done::Saved)
        }
        PutTo::Sound { id, over } => {
            due.arm(files, port, 0)?;
            let file = library_file(id);
            Card::new()
                .run(files, |s, r| {
                    if !over && exists(s, r, file)? {
                        return Err(PutFail::Exists);
                    }
                    session(s, r, Dir::Sounds, port, due, |s, port| {
                        let mut src = Incoming::new(arg, port, due);
                        save::<_, _, SoundCheck>(s, r, file, &mut src, |s, g| {
                            judge_pair::<_, SoundCheck>(s, r, file, g)
                        })
                    })
                })
                .and_then(|o| o.result)
                .map(|()| Done::Saved)
        }
        #[cfg(feature = "source-writer")]
        PutTo::Source(stem) => {
            let check = SourceCheck::new(stem, arg.size).map_err(PutFail::Source)?;
            due.arm(files, port, 0)?;
            Card::new()
                .run(files, |s, r| {
                    source_session(s, r, stem, check, arg, port, due)
                })
                .and_then(|o| o.result)
        }
        PutTo::Bank2 => {
            let seal = seal.ok_or(PutFail::Unsealed)?;
            due.arm(files, port, 0)?;
            Card::new()
                .run(files, |s, r| {
                    whole_session(s, r, Dir::System, STAGED, arg, port, due, |s, r, port| {
                        // The read back: a deadline of its own, as a source's.
                        Due(port.now()).arm(s, port, 0)?;
                        match chimera_image::judge(&mut CardImage::new(s, r.volume()), &seal) {
                            chimera_image::Verdict::Good(_) => Ok(Done::Saved),
                            chimera_image::Verdict::Stale { have } => Ok(Done::Stale(have)),
                            _ => Err(PutFail::NotBank2),
                        }
                    })
                })
                .and_then(|o| o.result)
        }
        PutTo::Beds | PutTo::Tuning(_) | PutTo::Map(_) => {
            let dir = match arg.to {
                PutTo::Tuning(_) => Dir::Tunings,
                PutTo::Map(_) => Dir::Maps,
                _ => Dir::Bench,
            };
            due.arm(files, port, 0)?;
            Card::new()
                .run(files, |s, r| {
                    let file = match arg.to {
                        PutTo::Tuning(f) => f,
                        // Never the live side: it goes once this one is
                        // whole and judged.
                        PutTo::Map(f) => {
                            let st =
                                |s: &mut F, sd| check_frame(s, r, f.side(sd), FileKind::Controller);
                            let (a, b) = (st(s, Side::A)?, st(s, Side::B)?);
                            f.side(write_target(a, b).map_err(PutFail::File)?.0)
                        }
                        _ => BEDS_FILE,
                    };
                    whole_session(s, r, dir, file, arg, port, due, |s, r, _| {
                        if dir == Dir::Tunings {
                            // A new scale drops its slot's old map: else it
                            // stays applied unseen (`put` the map after).
                            if file.ext() == b"SCL"
                                && let Some(kbm) = FileName::new(dir, file.stem(), b"KBM")
                            {
                                match s.delete(r.volume(), kbm) {
                                    Ok(()) | Err(StoreError::NotFound) => {}
                                    Err(e) => return Err(e.into()),
                                }
                            }
                            crate::tuning::file_put();
                        }
                        if dir == Dir::Maps {
                            map_judge(s, r, file)?;
                        }
                        Ok(Done::Saved)
                    })
                })
                .and_then(|o| o.result)
        }
    }
}

/// A whole file on the mounted card (bank 2's image, the bench's table):
/// `dir`, the old file deleted, `READY`, the data written as it comes, then
/// `judge`d. Any failure past `READY` leaves no file and drains the host
/// before the `ERR`.
#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn whole_session<F: Files, P: Out>(
    s: &mut F,
    r: &Ready,
    dir: Dir,
    file: FileName,
    arg: PutArg,
    port: &mut P,
    due: Due,
    judge: impl FnOnce(&mut F, &Ready, &mut P) -> Result<Done, PutFail>,
) -> Result<Done, PutFail> {
    for d in [Dir::Chimera, dir] {
        due.arm(s, port, 0)?;
        s.make_dir(r.volume(), d)?;
    }
    due.arm(s, port, 0)?;
    match s.delete(r.volume(), file) {
        Ok(()) | Err(StoreError::NotFound) => {}
        Err(e) => return Err(e.into()),
    }
    port.fresh();
    port.put(READY.as_bytes())
        .map_err(|Stalled| PutFail::Stalled)?;
    let mut src = Incoming::new(arg, port, due);
    let mut why = None;
    let wrote = src.due.arm(s, src.port, VERIFY_MS).and_then(|()| {
        s.write(r.volume(), file, &mut |sink| {
            copy_body(&mut src, &mut |_| Ok(()), &mut |b| sink.put(b)).map_err(|e| match e {
                PutFail::Store(e) => e,
                e => {
                    why = Some(e);
                    StoreError::Io
                }
            })
        })
        .map_err(|e| why.unwrap_or(PutFail::Store(e)))
    });
    let port = src.port;
    let done = wrote.and_then(|_| judge(s, r, port));
    if done.is_err() {
        s.cap(Some(VERIFY_MS));
        let _ = s.delete(r.volume(), file);
        drain(port);
    }
    done
}

/// A map's side just written: a controller map as a load frames one, its
/// header's name making its stem, else refused (and deleted); then the
/// pair's other side deleted, so this side loads, and the UI's layers read
/// again.
fn map_judge<F: Files>(s: &mut F, r: &Ready, file: FileName) -> Result<(), PutFail> {
    match check_frame(s, r, file, FileKind::Controller)? {
        SideState::Present { err: None, .. } => {}
        SideState::Present { err: Some(e), .. } | SideState::Torn(e) | SideState::Headerless(e) => {
            return Err(PutFail::File(e));
        }
        SideState::Missing => return Err(PutFail::File(FileError::Truncated)),
    }
    /// The file's first `HEADER_LEN` bytes.
    struct Head([u8; HEADER_LEN], usize);
    impl ReadSink for Head {
        fn begin(&mut self, _: u32) -> ControlFlow<()> {
            ControlFlow::Continue(())
        }
        fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
            let n = (HEADER_LEN - self.1).min(b.len());
            self.0[self.1..self.1 + n].copy_from_slice(&b[..n]);
            self.1 += n;
            match self.1 == HEADER_LEN {
                true => ControlFlow::Break(()),
                false => ControlFlow::Continue(()),
            }
        }
    }
    let mut h = Head([0; HEADER_LEN], 0);
    s.read(r.volume(), file, &mut h)?;
    let name = peek_header(&h.0).map_err(PutFail::File)?.name;
    match name.map(|n| crate::midi_map::stem(&n)) {
        Some((st, n)) if st.get(..n) == Some(file.stem()) => {}
        _ => return Err(PutFail::File(FileError::BadName)),
    }
    let other = if file.ext() == b"A" { b"B" } else { b"A" };
    if let Some(o) = FileName::new(Dir::Maps, file.stem(), other) {
        match s.delete(r.volume(), o) {
            Ok(()) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
    }
    crate::midi_map::map_put();
    Ok(())
}

/// After a failure past `READY`: the rest of the host's data read and
/// dropped, so the `ERR` reaches a host that has stopped sending.
fn drain(port: &mut impl Out) {
    let start = port.now();
    while port.now().since(start) < DRAIN_MS && port.recv(QUIET_MS).is_some() {}
}

/// On the mounted card: `dir`, `READY`, then the data to `save`. After
/// `READY` any failure drains the host before the `ERR`.
fn session<F: Files, P: Out>(
    s: &mut F,
    r: &Ready,
    dir: Dir,
    port: &mut P,
    due: Due,
    save: impl FnOnce(&mut F, &mut P) -> Result<(), PutFail>,
) -> Result<(), PutFail> {
    for d in [Dir::Chimera, dir] {
        due.arm(s, port, 0)?;
        s.make_dir(r.volume(), d)?;
    }
    port.fresh();
    port.put(READY.as_bytes())
        .map_err(|Stalled| PutFail::Stalled)?;
    let saved = save(s, port);
    if saved.is_err() {
        drain(port);
    }
    saved
}

/// A source on the mounted card: `TAPES`, then the file there judged
/// (`OK PRESENT` for one sealed and whole, `damaged` for one sealed and
/// not, a torn one deleted), `READY`, the chunks written as they come,
/// then read back and sealed. Any failure past `READY` deletes the
/// unsealed file and drains the host before the `ERR`.
#[cfg(feature = "source-writer")]
#[inline(never)]
fn source_session<F: Files, P: Out>(
    s: &mut F,
    r: &Ready,
    stem: Stem,
    mut check: SourceCheck,
    arg: PutArg,
    port: &mut P,
    due: Due,
) -> Result<Done, PutFail> {
    for d in [Dir::Chimera, Dir::Tapes] {
        due.arm(s, port, 0)?;
        s.make_dir(r.volume(), d)?;
    }
    due.arm(s, port, 0)?;
    if source_target(s, r, stem)? == Wrote::Present {
        return Ok(Done::Present);
    }
    port.fresh();
    port.put(READY.as_bytes())
        .map_err(|Stalled| PutFail::Stalled)?;
    let file = source_name(stem);
    let mut src = Incoming::new(arg, port, Due(port.now()));
    src.chunk = Some(Chunk::after(0, arg.size));
    src.reserve_ms = 0;
    // Each chunk ends within its deadline of the last one's end, so the
    // data is in by the chunks' deadlines from now.
    s.cap(Some(
        arg.size.div_ceil(PUT_CHUNK_BYTES) * PUT_DEADLINE_MS + SD_MARGIN_MS,
    ));
    let mut why = None;
    let wrote = s.write(r.volume(), file, &mut |sink| {
        let check = &mut |b: &[u8]| check.feed(b).map_err(PutFail::Source);
        copy_body(&mut src, check, &mut |b| sink.put(b)).map_err(|e| match e {
            PutFail::Store(e) => e,
            e => {
                why = Some(e);
                StoreError::Io
            }
        })
    });
    let port = src.port;
    let done = match wrote {
        Err(e) => Err(why.unwrap_or(PutFail::Store(e))),
        // The read back and the seal: a deadline of their own.
        Ok(_) => Due(port.now())
            .arm(s, port, 0)
            .and_then(|()| Ok(verify_and_seal(s, r, stem)?)),
    };
    if let Err(e) = done {
        // Unsealed: a torn or unchecked file never stays.
        s.cap(Some(VERIFY_MS));
        let _ = s.delete(r.volume(), file);
        drain(port);
        return Err(e);
    }
    Ok(Done::Saved)
}

/// The bytes, as they come, through `check` and then to `put` a block at
/// a time. The last block, a source's trailer in it, goes only once every
/// chunk, the host's CRC and the check passed: a put that fails leaves a
/// file no read takes, even before it is deleted.
fn copy_body<P: Out>(
    src: &mut Incoming<'_, P>,
    check: &mut dyn FnMut(&[u8]) -> Result<(), PutFail>,
    put: &mut dyn FnMut(&[u8]) -> Result<(), StoreError>,
) -> Result<(), PutFail> {
    let mut block = [0u8; CHUNK];
    loop {
        let mut fill = 0;
        while fill < CHUNK && src.got < src.size {
            block[fill] = src.next()?;
            fill += 1;
        }
        check(&block[..fill])?;
        if src.got == src.size {
            if src.all.finish() != src.crc {
                return Err(PutFail::BadCrc);
            }
            return Ok(put(&block[..fill])?);
        }
        put(&block[..fill])?;
        src.last_block = src.port.now();
    }
}

/// The chunk being read: its bytes' CRC so far, and where it ends.
#[cfg(feature = "source-writer")]
struct Chunk {
    crc: Crc32,
    end: u32,
}

#[cfg(feature = "source-writer")]
impl Chunk {
    /// The chunk from byte `start` of a `size`-byte file.
    fn after(start: u32, size: u32) -> Chunk {
        Chunk {
            crc: Crc32::new(),
            end: start.saturating_add(PUT_CHUNK_BYTES).min(size),
        }
    }
}

/// The header read, then the file saved as the unit saves it while the
/// kind's own check reads it as it comes: a file the check refuses
/// ends before its trailer, a torn side a load never takes. Then `judge`:
/// `OK` only if the pair shows the side just written as the one that loads.
#[inline(never)]
fn save<F: Files, P: Out, C: Check + Default>(
    s: &mut F,
    r: &Ready,
    file: AbFile,
    src: &mut Incoming<'_, P>,
    judge: impl FnOnce(&mut F, Generation) -> Result<(), PutFail>,
) -> Result<(), PutFail> {
    let mut framer = Framer::new(src.size).map_err(PutFail::File)?;
    let mut check = C::default();
    let mut head = [0; HEADER_LEN];
    for b in &mut head {
        *b = src.next()?;
    }
    let h = peek_header(&head).map_err(PutFail::File)?;
    if h.kind != C::KIND {
        return Err(PutFail::File(FileError::WrongKind));
    }
    framer
        .push(&head, &mut |e: Event<'_>| check.event(e))
        .map_err(PutFail::File)?;
    let (side, generation) = save_target(s, r, file, &mut C::default()).map_err(|e| match e {
        SaveError::Store(e) => PutFail::Store(e),
        SaveError::File(e) => PutFail::File(e),
        SaveError::Damaged => PutFail::File(FileError::Corrupt),
    })?;
    let h = Header { generation, ..h };
    src.due.arm(s, src.port, VERIFY_MS)?;
    let mut why = None;
    let wrote = s.write(r.volume(), file.side(side), &mut |sink| {
        write_file(sink, &h, &mut |w| {
            src.body(&mut |b| w.copy(b), &mut framer, &mut |e| check.event(e))
                .and_then(|()| check.end().map_err(PutFail::File))
                .map_err(|e| match e {
                    PutFail::Store(e) => e,
                    e => {
                        why = Some(e);
                        StoreError::Io
                    }
                })
        })
    });
    if let Err(e) = wrote {
        // The side is torn, or never got its trailer: gone, the list shows
        // the pair as it was.
        if src.due.arm(s, src.port, 0).is_ok() {
            let _ = s.delete(r.volume(), file.side(side));
        }
        return Err(why.unwrap_or(PutFail::Store(e)));
    }
    src.due.arm(s, src.port, 0)?;
    judge(s, generation)
}

/// `Ok` if the headers, as the project list reads them, show the new
/// generation as the newest readable side, with nothing to refuse.
fn judge<F: Files>(s: &mut F, r: &Ready, id: ProjectId, g: Generation) -> Result<(), PutFail> {
    match peek_entry(s, r.volume(), id)? {
        Some(e) if e.err.is_none() && e.generation == Some(g) => Ok(()),
        Some(e) => Err(PutFail::File(e.err.unwrap_or(FileError::Corrupt))),
        None => Err(PutFail::File(FileError::Corrupt)),
    }
}

/// `Ok` if a load of `file`, as `C` judges both sides, takes the side at
/// generation `g`: the kind's check after the write.
fn judge_pair<F: Files, C: Check + Default>(
    s: &mut F,
    r: &Ready,
    file: AbFile,
    g: Generation,
) -> Result<(), PutFail> {
    let mut c = C::default();
    let [a, b] = [Side::A, Side::B].map(|sd| {
        c.side(sd);
        check_file(s, r, file.side(sd), &mut c)
    });
    let (a, b) = (a?, b?);
    let loads = match pick(a, b) {
        Pick::Load(Side::A) => a,
        Pick::Load(Side::B) => b,
        Pick::Refuse(_, e) => return Err(PutFail::File(e)),
        Pick::Missing => return Err(PutFail::File(FileError::Corrupt)),
    };
    match loads {
        SideState::Present { generation, .. } if generation == g => Ok(()),
        _ => Err(PutFail::File(FileError::Corrupt)),
    }
}

/// The host's data: base64 lines, decoded a line at a time, the stall
/// measured from the last whole block handed on.
struct Incoming<'p, P: Out> {
    port: &'p mut P,
    /// The whole put's, or with chunks the chunk's.
    due: Due,
    /// Kept back from `due` for the card work after the data.
    reserve_ms: u32,
    /// A source's chunk; `None` for a project, which has none.
    #[cfg(feature = "source-writer")]
    chunk: Option<Chunk>,
    size: u32,
    crc: u32,
    line: [u8; PUT_LINE],
    out: [u8; DECODED],
    at: usize,
    len: usize,
    /// Bytes handed out, and their CRC.
    got: u32,
    all: Crc32,
    last_block: Ms,
}

impl<'p, P: Out> Incoming<'p, P> {
    fn new(arg: PutArg, port: &'p mut P, due: Due) -> Self {
        let last_block = port.now();
        Incoming {
            port,
            due,
            reserve_ms: VERIFY_MS + SD_MARGIN_MS,
            #[cfg(feature = "source-writer")]
            chunk: None,
            size: arg.size,
            crc: arg.crc,
            line: [0; PUT_LINE],
            out: [0; DECODED],
            at: 0,
            len: 0,
            got: 0,
            all: Crc32::new(),
            last_block,
        }
    }

    fn next(&mut self) -> Result<u8, PutFail> {
        if self.at == self.len {
            self.fill()?;
        }
        let b = *self.out.get(self.at).ok_or(PutFail::BadData)?;
        self.at += 1;
        self.got += 1;
        self.all.update(&[b]);
        Ok(b)
    }

    /// The rest of the file after the header, through `framer`: its
    /// records to `put` a block at a time, so the wait for the host never
    /// sits inside a card write. Then the trailer, the host's CRC, and the
    /// file's own verdict, each before the new side gets its trailer.
    fn body(
        &mut self,
        put: &mut dyn FnMut(&[u8]) -> Result<(), StoreError>,
        framer: &mut Framer,
        on: &mut dyn FnMut(Event<'_>) -> Result<(), FileError>,
    ) -> Result<(), PutFail> {
        let mut block = [0u8; CHUNK];
        let mut fill = 0;
        let records = self.size - 4;
        while self.got < self.size {
            block[fill] = self.next()?;
            fill += 1;
            if fill == CHUNK || self.got == records || self.got == self.size {
                framer.push(&block[..fill], on).map_err(PutFail::File)?;
                if self.got <= records {
                    put(&block[..fill])?;
                    self.last_block = self.port.now();
                }
                fill = 0;
            }
        }
        if self.all.finish() != self.crc {
            return Err(PutFail::BadCrc);
        }
        framer.finish().map_err(PutFail::File)
    }

    /// The next line, its length in `line`: within the deadline, the
    /// stall counted from the last block.
    fn read_line(&mut self) -> Result<usize, PutFail> {
        let mut n = 0;
        loop {
            if self.due.left_ms(self.port) <= self.reserve_ms {
                return Err(PutFail::Timeout);
            }
            let waited = self.port.now().since(self.last_block);
            let wait = PUT_STALL_MS.saturating_sub(waited);
            let b = match wait {
                0 => None,
                w => self.port.recv(w),
            }
            .ok_or(PutFail::Stalled)?;
            match b {
                b'\n' | b'\r' if n == 0 => {}
                b'\n' | b'\r' => break,
                b => {
                    *self.line.get_mut(n).ok_or(PutFail::BadData)? = b;
                    n += 1;
                }
            }
        }
        if self.line.get(..n) == Some(ABORT) {
            return Err(PutFail::Aborted);
        }
        Ok(n)
    }

    /// The next data line, decoded.
    fn fill(&mut self) -> Result<(), PutFail> {
        let n = self.read_line()?;
        let text = self.line.get(..n).unwrap_or(&[]);
        let (quads, rest) = text.as_chunks::<4>();
        if !rest.is_empty() {
            return Err(PutFail::BadData);
        }
        let last = quads.len().saturating_sub(1);
        let mut len = 0;
        for (i, q) in quads.iter().enumerate() {
            let (bytes, k) = quad(q, i == last).ok_or(PutFail::BadData)?;
            self.out[len..len + k].copy_from_slice(&bytes[..k]);
            len += k;
        }
        let end = self.got + len as u32;
        if end > self.size {
            return Err(PutFail::TooLong);
        }
        #[cfg(feature = "source-writer")]
        let boundary = match &self.chunk {
            Some(c) if end > c.end => return Err(PutFail::Chunk),
            Some(c) => c.end,
            None => self.size,
        };
        #[cfg(not(feature = "source-writer"))]
        let boundary = self.size;
        if len % 3 != 0 && end != boundary {
            // Padding ends the file, or a chunk, not a line.
            return Err(PutFail::BadData);
        }
        (self.at, self.len) = (0, len);
        #[cfg(feature = "source-writer")]
        if let Some(c) = &mut self.chunk {
            c.crc.update(&self.out[..len]);
            if end == c.end {
                self.close_chunk(end)?;
            }
        }
        Ok(())
    }

    /// The chunk's `end <crc32>` line, checked, then `ACK <bytes so far>`;
    /// the next chunk's deadline starts.
    #[cfg(feature = "source-writer")]
    fn close_chunk(&mut self, end: u32) -> Result<(), PutFail> {
        let n = self.read_line()?;
        let said = self
            .line
            .get(..n)
            .and_then(|l| l.strip_prefix(b"end "))
            .and_then(|h| number(h, 16, 8))
            .ok_or(PutFail::BadData)?;
        let c = self.chunk.as_mut().ok_or(PutFail::Chunk)?;
        if c.crc.finish() != said {
            return Err(PutFail::Chunk);
        }
        *c = Chunk::after(end, self.size);
        self.port.fresh();
        writeln!(super::Text(&mut *self.port), "ACK {end}")
            .map_err(|fmt::Error| PutFail::Stalled)?;
        self.due = Due(self.port.now());
        Ok(())
    }
}

/// Each byte's base64 value, 64 where it is not one.
static B64: [u8; 256] = {
    let mut t = [64u8; 256];
    let mut i = 0;
    while i < 64 {
        let c = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"[i];
        t[c as usize] = i as u8;
        i += 1;
    }
    t
};

/// One base64 quad: its bytes and how many. Padding only in the last.
fn quad(q: &[u8; 4], last: bool) -> Option<([u8; 3], usize)> {
    let pad = match q {
        [_, _, b'=', b'='] => 2,
        [_, _, _, b'='] => 1,
        _ => 0,
    };
    if pad > 0 && !last {
        return None;
    }
    let mut n = 0u32;
    for (i, &c) in q.iter().enumerate() {
        let x = if i >= 4 - pad { 0 } else { B64[usize::from(c)] };
        if x == 64 {
            return None;
        }
        n = n << 6 | u32::from(x);
    }
    let [_, a, b, c] = n.to_be_bytes();
    Some(([a, b, c], 3 - pad))
}

/// A name as the card shows it: `STEM.EXT`, or `STEM`.
struct Shown(FileName);

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = |b| core::str::from_utf8(b).unwrap_or("?");
        f.write_str(text(self.0.stem()))?;
        match self.0.ext() {
            [] => Ok(()),
            ext => write!(f, ".{}", text(ext)),
        }
    }
}

/// `ls`'s whole answer: one `NAME size` line a file in PROJECTS (or
/// TAPES), then `OK`, or `ERR ls: <why>`. A write that stalls ends the
/// listing's text, not the card operation.
#[inline(never)]
pub fn ls<F: Files>(arg: LsArg, f: &mut F, out: &mut impl Out) -> Result<(), Stalled> {
    let dir = match arg {
        LsArg::Projects => Dir::Projects,
        LsArg::Tapes => Dir::Tapes,
        LsArg::Sounds => Dir::Sounds,
        LsArg::Maps => Dir::Maps,
    };
    let mut t = super::Text(out);
    let mut wrote = Ok(());
    let listed = f.mount().and_then(|vol| {
        t.0.fresh();
        f.list(vol, dir, &mut |name, len| {
            if wrote.is_ok() {
                wrote = writeln!(t, "{} {len}", Shown(name));
            }
        })
    });
    match listed {
        Ok(()) => wrote.and_then(|()| t.write_str("OK\n")),
        Err(e) => writeln!(t, "ERR ls: {}", e.message()),
    }
    .map_err(|fmt::Error| Stalled)
}
