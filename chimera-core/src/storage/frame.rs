//! The file header, and the push `Framer` that checks a file as it streams.

use crate::name::Name;

use super::crc::Crc32;
use super::record::{CRITICAL, MAX_RECORD_LEN, ReadTag, RecordTag};

pub const MAGIC: [u8; 4] = *b"CHIM";
pub const FORMAT_VERSION: u16 = 1;
/// Magic 4, version `u16`, kind `u8`, flags `u8` (0), generation `u32`, name `[u8; 16]`.
pub const HEADER_LEN: usize = 28;
/// CRC32, little-endian, over the header and the records.
pub const TRAILER_LEN: usize = 4;
const RECORD_HEAD_LEN: usize = 4;

/// What a file holds. 2 Project, 4 Tags and 5 Index are reserved (ADR 0045).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FileKind {
    Sound = 1,
    System = 3,
}

impl FileKind {
    fn from_code(c: u8) -> Option<Self> {
        match c {
            1 => Some(FileKind::Sound),
            3 => Some(FileKind::System),
            _ => None,
        }
    }
}

/// A save counter that wraps: compare with `is_newer_than`, never `<`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Generation(u32);

impl Generation {
    pub const FIRST: Self = Generation(1);

    pub const fn new(n: u32) -> Self {
        Generation(n)
    }

    pub fn next(self) -> Self {
        Generation(self.0.wrapping_add(1))
    }

    /// Serial-number order: newer when at most 2³¹ − 1 saves ahead.
    pub fn is_newer_than(self, o: Self) -> bool {
        (self.0.wrapping_sub(o.0) as i32) > 0
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

/// Which file of an A/B pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

impl Side {
    /// The 8.3 extension.
    pub fn ext(self) -> &'static [u8] {
        match self {
            Side::A => b"A",
            Side::B => b"B",
        }
    }

    pub fn other(self) -> Side {
        match self {
            Side::A => Side::B,
            Side::B => Side::A,
        }
    }
}

/// A project's id, 1..=9 999 999, so its stem `P` + 7 digits is 8.3.
///
/// ```compile_fail,E0423
/// use chimera_core::storage::ProjectId;
/// let _ = ProjectId(5);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProjectId(u32);

impl ProjectId {
    pub const MAX: u32 = 9_999_999;

    pub fn new(n: u32) -> Option<Self> {
        (1..=Self::MAX).contains(&n).then_some(ProjectId(n))
    }

    pub fn get(self) -> u32 {
        self.0
    }

    pub fn stem(self) -> [u8; 8] {
        let mut s = *b"P0000000";
        let mut n = self.0;
        for d in s[1..].iter_mut().rev() {
            *d = b'0' + (n % 10) as u8;
            n /= 10;
        }
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: FileKind,
    pub generation: Generation,
    /// `None` on disk is 16 NULs.
    pub name: Option<Name<16>>,
}

impl Header {
    pub(super) fn encode(&self) -> [u8; HEADER_LEN] {
        let mut b = [0; HEADER_LEN];
        b[..4].copy_from_slice(&MAGIC);
        b[4..6].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
        b[6] = self.kind as u8;
        b[8..12].copy_from_slice(&self.generation.0.to_le_bytes());
        if let Some(n) = self.name {
            b[12..].copy_from_slice(&n.padded());
        }
        b
    }

    fn decode(b: &[u8; HEADER_LEN]) -> Result<Header, FileError> {
        if b[..4] != MAGIC {
            return Err(FileError::BadMagic);
        }
        match u16::from_le_bytes([b[4], b[5]]) {
            FORMAT_VERSION => {}
            0 => return Err(FileError::Corrupt),
            _ => return Err(FileError::NeedsNewerFirmware),
        }
        let kind = FileKind::from_code(b[6]).ok_or(FileError::WrongKind)?;
        if b[7] != 0 {
            return Err(FileError::Corrupt);
        }
        let generation = Generation(u32::from_le_bytes([b[8], b[9], b[10], b[11]]));
        let mut raw = [0; 16];
        raw.copy_from_slice(&b[12..]);
        let name = if raw == [0; 16] {
            None
        } else {
            Some(Name::from_padded(&raw).map_err(|_| FileError::BadName)?)
        };
        Ok(Header {
            kind,
            generation,
            name,
        })
    }
}

/// Why a file can't be read. A file error, never a card fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileError {
    Truncated,
    BadMagic,
    BadCrc,
    NeedsNewerFirmware,
    WrongKind,
    /// A length or count past its bound.
    Bounds,
    BadName,
    Corrupt,
}

impl FileError {
    /// The line the UI shows.
    pub fn message(self) -> &'static str {
        match self {
            FileError::Truncated => "FILE IS CUT SHORT",
            FileError::BadMagic => "NOT A CHIMERA FILE",
            FileError::BadCrc => "FILE CHECKSUM FAILED",
            FileError::NeedsNewerFirmware => "NEEDS NEWER FIRMWARE",
            FileError::WrongKind => "WRONG FILE TYPE",
            FileError::Bounds => "FILE DATA OUT OF BOUNDS",
            FileError::BadName => "FILE NAME IS INVALID",
            FileError::Corrupt => "FILE IS DAMAGED",
        }
    }

    /// A side a cut write could have left: its bytes fail the CRC, or stop
    /// short of the length. Every other error is a verdict on bytes whose CRC
    /// matched, because the framer defers every verdict until the CRC is known.
    pub fn is_torn(self) -> bool {
        matches!(self, FileError::Truncated | FileError::BadCrc)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event<'a> {
    Header(Header),
    /// An unknown tag is non-critical (the framer refuses a critical one) and
    /// comes with an empty payload: the framer skips it by its length.
    Record(ReadTag, &'a [u8]),
}

#[derive(Clone, Copy, Debug)]
enum State {
    Header,
    RecordHead,
    RecordBody {
        tag: RecordTag,
        len: usize,
    },
    /// Hashed, not parsed: an unknown record, or the rest of the body once a
    /// verdict is in.
    Skip {
        len: usize,
    },
    Trailer,
    Done,
    Failed(FileError),
}

/// Checks a file pushed in chunks of any size, handing out the header, then
/// each record.
///
/// Events are provisional: the CRC is only known at `finish`, so a decoder
/// applies what it heard only after `finish` is Ok (a second pass). Every
/// verdict waits for the CRC too. The first error, the framer's or the
/// callback's, stops the events; the rest of the body is hashed unparsed, and
/// `finish` gives `BadCrc` if the CRC fails, else that error. So one flipped
/// bit reads as torn, never as a verdict (`NeedsNewerFirmware`) that would
/// shadow the other A/B side.
pub struct Framer {
    state: State,
    /// The first error; `finish` gives it once the CRC matches.
    verdict: Option<FileError>,
    crc: Crc32,
    pos: usize,
    file_len: usize,
    /// Bytes of the header, record head or trailer in progress.
    head: [u8; HEADER_LEN],
    /// Bytes of the current state's unit received so far.
    have: usize,
    rec_buf: [u8; MAX_RECORD_LEN],
}

impl Framer {
    /// `file_len` is the length the store reports. Under 32 B there is no
    /// header and trailer to check, so the file is `Truncated`: torn.
    pub fn new(file_len: u32) -> Result<Self, FileError> {
        let file_len = file_len as usize;
        if file_len < HEADER_LEN + TRAILER_LEN {
            return Err(FileError::Truncated);
        }
        Ok(Framer {
            state: State::Header,
            verdict: None,
            crc: Crc32::new(),
            pos: 0,
            file_len,
            head: [0; HEADER_LEN],
            have: 0,
            rec_buf: [0; MAX_RECORD_LEN],
        })
    }

    /// Errs only on bytes past `file_len` (`Corrupt`, sticky): the stream
    /// disagrees with its own length. Every verdict on the file comes from
    /// `finish`.
    pub fn push(
        &mut self,
        chunk: &[u8],
        on: &mut dyn FnMut(Event<'_>) -> Result<(), FileError>,
    ) -> Result<(), FileError> {
        if let State::Failed(e) = self.state {
            return Err(e);
        }
        let r = self.run(chunk, on);
        if let Err(e) = r {
            self.state = State::Failed(e);
        }
        r
    }

    /// `Truncated` short of `file_len`; else `BadCrc` if the trailer doesn't
    /// match; else the first deferred error, if any.
    pub fn finish(&self) -> Result<(), FileError> {
        match self.state {
            State::Failed(e) => Err(e),
            State::Done => {
                let t = &self.head[..TRAILER_LEN];
                if u32::from_le_bytes([t[0], t[1], t[2], t[3]]) != self.crc.finish() {
                    return Err(FileError::BadCrc);
                }
                self.verdict.map_or(Ok(()), Err)
            }
            _ => Err(FileError::Truncated),
        }
    }

    /// Record the first verdict, then hash the rest of the body unparsed.
    fn defer(&mut self, e: FileError) {
        self.verdict.get_or_insert(e);
        match self.body_end() - self.pos {
            0 => self.start(State::Trailer),
            len => self.start(State::Skip { len }),
        }
    }

    fn body_end(&self) -> usize {
        self.file_len - TRAILER_LEN
    }

    fn run(
        &mut self,
        mut chunk: &[u8],
        on: &mut dyn FnMut(Event<'_>) -> Result<(), FileError>,
    ) -> Result<(), FileError> {
        while !chunk.is_empty() {
            let unit = match self.state {
                State::Header => HEADER_LEN,
                State::RecordHead => RECORD_HEAD_LEN,
                State::RecordBody { len, .. } | State::Skip { len } => len,
                State::Trailer => TRAILER_LEN,
                State::Done => return Err(FileError::Corrupt),
                State::Failed(e) => return Err(e),
            };
            let n = (unit - self.have).min(chunk.len());
            let (take, rest) = chunk.split_at(n);
            chunk = rest;
            let at = self.have..self.have + n;
            match self.state {
                State::RecordBody { .. } => self.rec_buf[at].copy_from_slice(take),
                State::Skip { .. } => {}
                _ => self.head[at].copy_from_slice(take),
            }
            if !matches!(self.state, State::Trailer) {
                self.crc.update(take);
            }
            self.pos += n;
            self.have += n;
            if self.have == unit
                && let Err(e) = self.complete(on)
            {
                self.defer(e);
            }
        }
        Ok(())
    }

    /// The current unit is whole: act on it and move on.
    fn complete(
        &mut self,
        on: &mut dyn FnMut(Event<'_>) -> Result<(), FileError>,
    ) -> Result<(), FileError> {
        match self.state {
            State::Header => {
                on(Event::Header(Header::decode(&self.head)?))?;
                self.next_record()
            }
            State::RecordHead => {
                let code = u16::from_le_bytes([self.head[0], self.head[1]]);
                let len = u16::from_le_bytes([self.head[2], self.head[3]]) as usize;
                let tag = RecordTag::from_code(code);
                if tag.is_none() && code & CRITICAL != 0 {
                    return Err(FileError::NeedsNewerFirmware);
                }
                if len > self.body_end() - self.pos {
                    return Err(FileError::Bounds);
                }
                match tag {
                    Some(_) if len > MAX_RECORD_LEN => Err(FileError::Bounds),
                    Some(tag) if len > 0 => {
                        self.start(State::RecordBody { tag, len });
                        Ok(())
                    }
                    Some(tag) => {
                        on(Event::Record(ReadTag::Known(tag), &[]))?;
                        self.next_record()
                    }
                    None => {
                        on(Event::Record(ReadTag::Unknown(code), &[]))?;
                        if len > 0 {
                            self.start(State::Skip { len });
                            Ok(())
                        } else {
                            self.next_record()
                        }
                    }
                }
            }
            State::RecordBody { tag, len } => {
                on(Event::Record(ReadTag::Known(tag), &self.rec_buf[..len]))?;
                self.next_record()
            }
            State::Skip { .. } => self.next_record(),
            State::Trailer => {
                self.state = State::Done;
                Ok(())
            }
            State::Done | State::Failed(_) => Ok(()),
        }
    }

    fn start(&mut self, s: State) {
        self.state = s;
        self.have = 0;
    }

    /// Another record head, or the trailer when the body is spent.
    fn next_record(&mut self) -> Result<(), FileError> {
        match self.body_end() - self.pos {
            0 => self.start(State::Trailer),
            left if left < RECORD_HEAD_LEN => return Err(FileError::Bounds),
            _ => self.start(State::RecordHead),
        }
        Ok(())
    }
}
