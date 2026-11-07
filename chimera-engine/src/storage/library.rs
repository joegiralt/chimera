//! The build a library Sound was saved on (library spec § Files).

use core::fmt;
use core::num::NonZeroU8;

use chimera_hal::store::{Dir, FileName, StoreError};

use super::file::{AbFile, Check, DecodeInPlace};
use super::frame::{Event, FileError, FileKind, Generation, HEADER_LEN, peek_header};
use super::record::{ReadTag, RecordTag, RecordWriter};
use super::sound::{SoundCheck, encode_sound_body};
use crate::block::DiskCode;
use crate::midi_map::KeptMap;
use crate::name::SoundName;
use crate::params::EngineType;
use crate::preset::Sound;

const VERSION_LEN: usize = 12;
const HASH_LEN: usize = 8;
/// `Build` payload: version, hash, a flags byte (bit 0 dirty).
pub const BUILD_LEN: usize = VERSION_LEN + HASH_LEN + 1;

/// A build's identity, NUL-padded as stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildStamp {
    version: [u8; VERSION_LEN],
    hash: [u8; HASH_LEN],
    dirty: bool,
}

const fn pad<const N: usize>(s: &str) -> [u8; N] {
    let b = s.as_bytes();
    assert!(b.len() <= N, "build stamp field too long");
    let mut out = [0; N];
    let mut i = 0;
    while i < b.len() {
        out[i] = b[i];
        i += 1;
    }
    out
}

/// Stored bytes up to the first NUL, a non-printable one as `?`.
fn clean<const N: usize>(p: &[u8]) -> [u8; N] {
    let mut out = [0; N];
    for (o, &c) in out.iter_mut().zip(p).take_while(|(_, c)| **c != 0) {
        *o = if (0x20..0x7F).contains(&c) { c } else { b'?' };
    }
    out
}

fn text(b: &[u8]) -> &str {
    let n = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    // Printable ASCII by construction (`new` takes a `&str`, `parse` cleans).
    core::str::from_utf8(&b[..n]).unwrap_or("?")
}

impl BuildStamp {
    pub const fn new(version: &str, hash: &str, dirty: bool) -> BuildStamp {
        BuildStamp {
            version: pad(version),
            hash: pad(hash),
            dirty,
        }
    }

    pub fn to_bytes(&self) -> [u8; BUILD_LEN] {
        let mut b = [0; BUILD_LEN];
        b[..VERSION_LEN].copy_from_slice(&self.version);
        b[VERSION_LEN..VERSION_LEN + HASH_LEN].copy_from_slice(&self.hash);
        b[BUILD_LEN - 1] = self.dirty as u8;
        b
    }

    /// `None` under `BUILD_LEN` bytes; the rest is read leniently.
    pub fn parse(p: &[u8]) -> Option<BuildStamp> {
        (p.len() >= BUILD_LEN).then(|| BuildStamp {
            version: clean(&p[..VERSION_LEN]),
            hash: clean(&p[VERSION_LEN..VERSION_LEN + HASH_LEN]),
            dirty: p[BUILD_LEN - 1] & 1 != 0,
        })
    }

    pub fn version(&self) -> &str {
        text(&self.version)
    }

    pub fn hash(&self) -> &str {
        text(&self.hash)
    }

    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Both clean, one real hash: a `dev` or `unknown` build names no commit.
    pub fn is_this_build(&self, this: &BuildStamp) -> bool {
        self.hash == this.hash
            && !self.dirty
            && !this.dirty
            && !matches!(self.hash(), "dev" | "unknown")
    }
}

/// `0.1.0 02e440e2`, with `+` after the hash when dirty.
impl fmt::Display for BuildStamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.version(), self.hash())?;
        if self.dirty {
            f.write_str("+")?;
        }
        Ok(())
    }
}

/// A factory Sound's description: 1-40 B of `A`-`Z`, `0`-`9`, space and
/// `. , ' - + / : ( ) & %`. Width is the host's check (`draw::text_width`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Note([u8; Note::MAX], NonZeroU8);

impl Note {
    pub const MAX: usize = 40;

    /// A description to write: `None` for any character outside the set.
    /// A stored one is read leniently (`peek_library`).
    pub fn new(t: &str) -> Option<Note> {
        let ok =
            |c: u8| c.is_ascii_uppercase() || c.is_ascii_digit() || b" .,'-+/:()&%".contains(&c);
        (t.bytes().all(ok))
            .then(|| Note::parse(t.as_bytes()))
            .flatten()
    }

    /// A stored payload, leniently: 1-40 B, a byte outside 0x20-0x7E as `?`.
    fn parse(p: &[u8]) -> Option<Note> {
        if p.len() > Self::MAX {
            return None;
        }
        // 1..=40: a `u8`.
        let len = NonZeroU8::new(p.len() as u8)?;
        let mut b = [0; Self::MAX];
        for (o, &c) in b.iter_mut().zip(p) {
            *o = if (0x20..0x7F).contains(&c) { c } else { b'?' };
        }
        Some(Note(b, len))
    }

    pub fn as_str(&self) -> &str {
        // Printable ASCII by construction.
        core::str::from_utf8(&self.0[..usize::from(self.1.get())]).unwrap_or("?")
    }
}

/// `Engine`, `Build`, `Note` if any, then the rest of the Sound.
pub fn encode_library_sound(
    s: &Sound,
    stamp: &BuildStamp,
    note: Option<&Note>,
    w: &mut RecordWriter<'_>,
) -> Result<(), StoreError> {
    w.put(RecordTag::Engine, &[s.engine().disk_code()])?;
    w.put(RecordTag::Build, &stamp.to_bytes())?;
    if let Some(n) = note {
        w.put(RecordTag::Note, n.as_str().as_bytes())?;
    }
    encode_sound_body(s, w)
}

/// `encode_library_sound`, then `kept`'s record while the Sound has no
/// map: a Part's, saved with bank 2's door closed (#862).
pub fn encode_library_part(
    s: &Sound,
    kept: &KeptMap,
    stamp: &BuildStamp,
    w: &mut RecordWriter<'_>,
) -> Result<(), StoreError> {
    encode_library_sound(s, stamp, None, w)?;
    if s.map.is_empty() {
        crate::midi_in::put_kept(kept, w)?;
    }
    Ok(())
}

/// A user Sound's number, 1..=8_999_999: the only kind a save can take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserId(u32);

impl UserId {
    pub fn new(n: u32) -> Option<UserId> {
        (1..=LibId::USER_MAX).contains(&n).then_some(UserId(n))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

/// A factory Sound's number, 9_000_001..=9_999_999: installed, never saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FactoryId(u32);

impl FactoryId {
    pub fn new(n: u32) -> Option<FactoryId> {
        (LibId::FACTORY_MIN..=9_999_999)
            .contains(&n)
            .then_some(FactoryId(n))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

/// `S9100001`: always seven digits, by its range.
impl core::fmt::Display for FactoryId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "S{}", self.0)
    }
}

/// A library Sound's number: `S#######`, a user or a factory id by construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct LibId(u32);

/// Which side of the split a `LibId` is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibKind {
    User(UserId),
    Factory(FactoryId),
}

impl From<UserId> for LibId {
    fn from(id: UserId) -> LibId {
        LibId(id.0)
    }
}

impl From<FactoryId> for LibId {
    fn from(id: FactoryId) -> LibId {
        LibId(id.0)
    }
}

impl LibId {
    pub const USER_MAX: u32 = 8_999_999;
    pub const FACTORY_MIN: u32 = 9_000_001;

    pub fn new(n: u32) -> Option<LibId> {
        UserId::new(n)
            .map(LibId::from)
            .or_else(|| FactoryId::new(n).map(LibId::from))
    }

    /// `S` and 7 digits, nothing else.
    pub fn parse(s: &str) -> Option<LibId> {
        let digits = s.strip_prefix('S')?;
        if digits.len() != 7 || !digits.bytes().all(|d| d.is_ascii_digit()) {
            return None;
        }
        LibId::new(digits.parse().ok()?)
    }

    pub fn get(self) -> u32 {
        self.0
    }

    pub fn kind(self) -> LibKind {
        // In range by construction: every `LibId` came from a `UserId` or a `FactoryId`.
        if self.is_factory() {
            LibKind::Factory(FactoryId(self.0))
        } else {
            LibKind::User(UserId(self.0))
        }
    }

    pub fn is_factory(self) -> bool {
        self.0 >= LibId::FACTORY_MIN
    }

    /// `S` and 7 digits.
    pub fn stem(self) -> [u8; 8] {
        let mut s = [b'S'; 8];
        let mut n = self.get();
        for d in s[1..].iter_mut().rev() {
            *d = b'0' + (n % 10) as u8;
            n /= 10;
        }
        s
    }
}

/// A library pair for reads, peeks, `rm` and console `put`; writes go through `user_file`.
pub fn library_file(id: LibId) -> AbFile {
    AbFile::new(Dir::Sounds, &id.stem()).expect("S + 7 digits is a stem")
}

/// The pair a library save writes: a user id only.
///
/// ```compile_fail,E0308
/// use chimera_engine::storage::{FactoryId, user_file};
/// let _ = user_file(FactoryId::new(9_100_001).unwrap());
/// ```
pub fn user_file(id: UserId) -> AbFile {
    library_file(id.into())
}

/// A `S#######.A` or `.B` in `SOUNDS`.
pub fn lib_id(f: FileName) -> Option<LibId> {
    let (Dir::Sounds, &[b'S', ref digits @ ..], b"A" | b"B") = (f.dir(), f.stem(), f.ext()) else {
        return None;
    };
    if digits.len() != 7 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    LibId::new(digits.iter().fold(0, |n, d| n * 10 + u32::from(d - b'0')))
}

/// Ids one window pass covers; its bitmap is `ID_WINDOW / 8` bytes.
pub const ID_WINDOW: u32 = 4096;

#[derive(Debug, PartialEq)]
pub enum NextId {
    Id(UserId),
    /// The window had none: scan the one at this base.
    Scan(u32),
    NoIds,
}

/// The id a save takes: `top + 1`, else the lowest clear bit of the window
/// at `window_base` (bit i is id `window_base + i`), else the next window.
pub fn next_user_id(top: u32, window_base: u32, taken: &[u8; 512]) -> NextId {
    if top < LibId::USER_MAX {
        return UserId::new(top + 1).map_or(NextId::NoIds, NextId::Id);
    }
    let clear = (0..ID_WINDOW).find(|&i| taken[i as usize / 8] >> (i % 8) & 1 == 0);
    if let Some(i) = clear {
        return UserId::new(window_base + i).map_or(NextId::NoIds, NextId::Id);
    }
    let next = window_base + ID_WINDOW;
    if next > LibId::USER_MAX {
        NextId::NoIds
    } else {
        NextId::Scan(next)
    }
}

/// Bytes a peek reads: the header, `Engine`, `Build` and a full `Note`
/// (28 + 5 + 25 + 4 + 40 = 102).
pub const LIB_PEEK: usize = 128;

/// What a list row shows, from a file's first bytes.
#[derive(Debug, PartialEq)]
pub struct Peek {
    pub generation: Generation,
    pub name: Option<SoundName>,
    pub engine: Option<EngineType>,
    pub stamp: Option<BuildStamp>,
    pub note: Option<Note>,
    /// An `Engine` code this build doesn't know: greyed, name and stamp kept.
    pub needs_newer: bool,
}

/// The record at `at`: its tag, payload and where the next record starts
/// (from its declared length), when whole in the file (before the CRC
/// trailer of a `file_len` file). The payload is cut at the end of `b`: a
/// record grown by a later build still shows its leading bytes.
fn record_at(b: &[u8], at: usize, file_len: u32) -> Option<(u16, &[u8], usize)> {
    let h = b.get(at..at + 4)?;
    let len = usize::from(u16::from_le_bytes([h[2], h[3]]));
    let end = at + 4 + len;
    let whole = end + super::frame::TRAILER_LEN <= file_len as usize;
    whole
        .then(|| b.get(at + 4..end.min(b.len())))
        .flatten()
        .map(|p| (u16::from_le_bytes([h[0], h[1]]), p, end))
}

/// A library Sound's header, engine, stamp and description (`Note`) from
/// its first `LIB_PEEK` bytes; no CRC check.
pub fn peek_library(b: &[u8], file_len: u32) -> Result<Peek, FileError> {
    let head: &[u8; HEADER_LEN] = b
        .get(..HEADER_LEN)
        .and_then(|h| h.try_into().ok())
        .filter(|_| file_len as usize >= HEADER_LEN + 4)
        .ok_or(FileError::Truncated)?;
    let h = peek_header(head)?;
    if h.kind != FileKind::Sound {
        return Err(FileError::WrongKind);
    }
    let mut peek = Peek {
        generation: h.generation,
        name: h.name,
        engine: None,
        stamp: None,
        note: None,
        needs_newer: false,
    };
    if let Some((tag, p, next)) = record_at(b, HEADER_LEN, file_len)
        && tag == RecordTag::Engine.code()
    {
        let &[code] = p else {
            return Err(FileError::Corrupt);
        };
        peek.engine = EngineType::from_disk_code(code);
        peek.needs_newer = peek.engine.is_none();
        let build = record_at(b, next, file_len).filter(|&(t, ..)| t == RecordTag::Build.code());
        peek.stamp = build.and_then(|(_, p, _)| BuildStamp::parse(p));
        // Third, straight after `Build` at its declared length (a later
        // build's longer one included); anything else is not a description.
        peek.note = build
            .and_then(|(.., next)| record_at(b, next, file_len))
            .filter(|&(t, ..)| t == RecordTag::Note.code())
            .and_then(|(_, p, _)| Note::parse(p));
    }
    Ok(peek)
}

/// A library Sound decoded in place onto `target` (`load_ab_in_place`):
/// pass 2 writes it only after a clean pass 1.
pub struct LibraryDecoder<'a> {
    check: SoundCheck,
    target: &'a mut Sound,
    /// The map record a closed door leaves unread (#862).
    kept: &'a mut KeptMap,
}

impl<'a> LibraryDecoder<'a> {
    pub fn new(target: &'a mut Sound, kept: &'a mut KeptMap) -> Self {
        kept.clear();
        LibraryDecoder {
            check: SoundCheck::new(),
            target,
            kept,
        }
    }

    /// Entries pass 2 dropped (`SoundCheck::skips`).
    pub fn skips(&self) -> u16 {
        self.check.skips()
    }

    /// `SoundCheck::over_budget`.
    pub fn over_budget(&self) -> bool {
        self.check.over_budget()
    }
}

impl Check for LibraryDecoder<'_> {
    const KIND: FileKind = FileKind::Sound;

    fn event(&mut self, e: Event<'_>) -> Result<(), FileError> {
        self.check.event(e)
    }

    fn end(&mut self) -> Result<(), FileError> {
        self.check.end()
    }
}

impl DecodeInPlace for LibraryDecoder<'_> {
    fn apply(&mut self, e: Event<'_>) -> Result<(), FileError> {
        if let Event::Record(ReadTag::Known(t), p) = e {
            crate::midi_in::keep_map(self.kept, t, p);
        }
        self.check.step(e, Some(self.target))
    }

    fn finish(&mut self) -> Result<(), FileError> {
        self.check.finish(Some(self.target))
    }
}
