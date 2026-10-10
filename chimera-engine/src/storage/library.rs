//! The build a library Sound was saved on (library spec § Files).

use core::fmt;

use chimera_hal::store::{Dir, FileName, StoreError};

use super::file::{AbFile, Check, DecodeInPlace};
use super::frame::{Event, FileError, FileKind, Generation, HEADER_LEN, peek_header};
use super::record::{RecordTag, RecordWriter};
use super::sound::{SoundCheck, encode_sound_body};
use crate::block::DiskCode;
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

/// `Engine`, `Build`, then the rest of the Sound.
pub fn encode_library_sound(
    s: &Sound,
    stamp: &BuildStamp,
    w: &mut RecordWriter<'_>,
) -> Result<(), StoreError> {
    w.put(RecordTag::Engine, &[s.engine().disk_code()])?;
    w.put(RecordTag::Build, &stamp.to_bytes())?;
    encode_sound_body(s, w)
}

/// A library Sound's number: `S#######`. User ids are 1..=8_999_999,
/// factory ids 9_000_001..=9_999_999; a save never takes a factory one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LibId(u32);

impl LibId {
    pub const USER_MAX: u32 = 8_999_999;
    pub const FACTORY_MIN: u32 = 9_000_001;

    pub fn new(n: u32) -> Option<LibId> {
        ((1..=9_999_999).contains(&n) && n != 9_000_000).then_some(LibId(n))
    }

    pub fn get(self) -> u32 {
        self.0
    }

    pub fn is_factory(self) -> bool {
        self.0 >= Self::FACTORY_MIN
    }

    /// `S` and 7 digits.
    pub fn stem(self) -> [u8; 8] {
        let mut s = [b'S'; 8];
        let mut n = self.0;
        for d in s[1..].iter_mut().rev() {
            *d = b'0' + (n % 10) as u8;
            n /= 10;
        }
        s
    }
}

pub fn library_file(id: LibId) -> AbFile {
    AbFile::new(Dir::Sounds, &id.stem()).expect("S + 7 digits is a stem")
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
    Id(LibId),
    /// The window had none: scan the one at this base.
    Scan(u32),
    NoIds,
}

/// The id a save takes: `top + 1`, else the lowest clear bit of the window
/// at `window_base` (bit i is id `window_base + i`), else the next window.
pub fn next_user_id(top: u32, window_base: u32, taken: &[u8; 512]) -> NextId {
    if top < LibId::USER_MAX {
        return LibId::new(top + 1).map_or(NextId::NoIds, NextId::Id);
    }
    let clear = (0..ID_WINDOW).find(|&i| taken[i as usize / 8] >> (i % 8) & 1 == 0);
    if let Some(i) = clear {
        return match LibId::new(window_base + i) {
            Some(id) if id.get() <= LibId::USER_MAX => NextId::Id(id),
            _ => NextId::NoIds,
        };
    }
    let next = window_base + ID_WINDOW;
    if next > LibId::USER_MAX {
        NextId::NoIds
    } else {
        NextId::Scan(next)
    }
}

/// Bytes a peek reads: the header, `Engine` and `Build`.
pub const LIB_PEEK: usize = 64;

/// What a list row shows, from a file's first bytes.
#[derive(Debug, PartialEq)]
pub struct Peek {
    pub generation: Generation,
    pub name: Option<SoundName>,
    pub engine: Option<EngineType>,
    pub stamp: Option<BuildStamp>,
    /// An `Engine` code this build doesn't know: greyed, name and stamp kept.
    pub needs_newer: bool,
}

/// The record at `at`: its tag and payload, when whole in `b` and before
/// the CRC trailer of a `file_len` file.
fn record_at(b: &[u8], at: usize, file_len: u32) -> Option<(u16, &[u8])> {
    let h = b.get(at..at + 4)?;
    let len = usize::from(u16::from_le_bytes([h[2], h[3]]));
    let end = at + 4 + len;
    let whole = end + super::frame::TRAILER_LEN <= file_len as usize;
    whole
        .then(|| b.get(at + 4..end))
        .flatten()
        .map(|p| (u16::from_le_bytes([h[0], h[1]]), p))
}

/// A library Sound's header, engine and stamp from its first `LIB_PEEK`
/// bytes; no CRC check.
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
        needs_newer: false,
    };
    if let Some((tag, p)) = record_at(b, HEADER_LEN, file_len)
        && tag == RecordTag::Engine.code()
    {
        let &[code] = p else {
            return Err(FileError::Corrupt);
        };
        peek.engine = EngineType::from_disk_code(code);
        peek.needs_newer = peek.engine.is_none();
        peek.stamp = record_at(b, HEADER_LEN + 5, file_len)
            .filter(|&(t, _)| t == RecordTag::Build.code())
            .and_then(|(_, p)| BuildStamp::parse(p));
    }
    Ok(peek)
}

/// A library Sound decoded in place onto `target` (`load_ab_in_place`):
/// pass 2 writes it only after a clean pass 1.
pub struct LibraryDecoder<'a> {
    check: SoundCheck,
    target: &'a mut Sound,
}

impl<'a> LibraryDecoder<'a> {
    pub fn new(target: &'a mut Sound) -> Self {
        LibraryDecoder {
            check: SoundCheck::new(),
            target,
        }
    }

    /// Entries pass 2 dropped (`SoundCheck::skips`).
    pub fn skips(&self) -> u16 {
        self.check.skips()
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
        self.check.step(e, Some(self.target))
    }

    fn finish(&mut self) -> Result<(), FileError> {
        self.check.finish(Some(self.target))
    }
}
