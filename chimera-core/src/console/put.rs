//! `ls` and `put`: project files onto a card that can't leave the unit.
//! `put` takes a whole project file and saves it the way the unit saves
//! (ADR 0045): to the side a load doesn't keep, with the next generation
//! and its own CRC, so the old side stays loadable until the new one is
//! whole. A side a failed `put` wrote is deleted, so the list shows the
//! pair as it was. Every card operation runs under what is left of
//! `PUT_DEADLINE_MS`.

use core::fmt::{self, Write};

use chimera_hal::Ms;
use chimera_hal::store::{CHUNK, Dir, FileName, Files, StoreError};

use super::{Arg, Out, Stalled, Words};
use crate::project::{ProjectCheck, peek_entry, project_file};
use crate::storage::{
    Card, CardFault, Check, Crc32, Event, FileError, Framer, Generation, HEADER_LEN, Header,
    ProjectId, Ready, SaveError, peek_header, save_target, write_file,
};

/// The largest file `put` takes: a project is about 130 KB.
pub const PUT_MAX_BYTES: u32 = 1 << 20;
/// The whole `put`, from its line to its answer, card work included.
pub const PUT_DEADLINE_MS: u32 = 30_000;
/// Each 512-byte block of data must arrive within this of the last.
pub const PUT_STALL_MS: u32 = 500;
/// Base64 characters a data line holds at most: 48 bytes.
pub const PUT_LINE: usize = 64;
/// The line that ends a transfer early.
pub const ABORT: &[u8] = b"abort";
/// What the unit says once it will take the data.
pub const READY: &str = "READY\n";
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

const _: () = assert!(PUT_LINE.is_multiple_of(4));
const _: () = assert!(PUT_LINE <= super::MAX_LINE);

/// Digits only, at most `max` of them, in `radix`.
fn number(w: &[u8], radix: u32, max: usize) -> Option<u32> {
    if w.is_empty() || w.len() > max {
        return None;
    }
    w.iter().try_fold(0u32, |n, &c| {
        let d = char::from(c).to_digit(radix)?;
        n.checked_mul(radix)?.checked_add(d)
    })
}

/// `/CHIMERA/PROJECTS/P0000014.A` (or `.B`: the letter names the pair; the
/// unit picks the side it writes), any case. Nothing else parses: no other
/// directory, no `..`, no `//`.
fn project(p: &[u8]) -> Option<ProjectId> {
    let (dir, name) = p.split_at_checked(PROJECTS.len())?;
    let [b'P' | b'p', digits @ .., b'.', b'A' | b'B' | b'a' | b'b'] = name else {
        return None;
    };
    if !dir.eq_ignore_ascii_case(PROJECTS) || digits.len() != 7 {
        return None;
    }
    ProjectId::new(number(digits, 10, 7)?)
}

/// `put`'s argument: the project, how many bytes and their
/// CRC-32/ISO-HDLC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PutArg {
    pub to: ProjectId,
    pub size: u32,
    pub crc: u32,
}

impl Arg for PutArg {
    const USAGE: &'static str = "/CHIMERA/PROJECTS/P#######.A, its size and CRC-32 in hex";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let to = project(words.next()?)?;
        let size = number(words.next()?, 10, 10)?;
        let crc = number(words.next()?, 16, 8)?;
        words.next().is_none().then_some(PutArg { to, size, crc })
    }
}

/// Why a `put` failed: `ERR put: <message>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutFail {
    ProjectOpen,
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
}

impl PutFail {
    pub fn message(self) -> &'static str {
        match self {
            PutFail::ProjectOpen => "project is open",
            PutFail::TooBig => "over 1 MiB",
            PutFail::Timeout => "timeout",
            PutFail::Stalled => "no block for 500 ms",
            PutFail::Aborted => "aborted",
            PutFail::BadData => "bad data line",
            PutFail::TooLong => "more data than its size",
            PutFail::BadCrc => "CRC mismatch",
            PutFail::File(e) => e.message(),
            PutFail::Store(e) => e.message(),
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

/// `put`'s whole answer: `READY` once its directory is made, then `OK`, or `ERR put: <why>` with the pair loading as before.
/// `open`: the project the UI holds, which `put` never writes under it.
/// Out of line, like `save`: 460 B smaller under fat LTO, measured.
#[inline(never)]
pub fn put<F: Files, P: Out>(
    arg: PutArg,
    open: Option<ProjectId>,
    files: &mut F,
    port: &mut P,
) -> Result<(), Stalled> {
    let due = Due(port.now());
    let r = transfer(arg, open, files, port, due);
    files.cap(None);
    port.fresh();
    let mut t = super::Text(port);
    match r {
        Ok(()) => t.write_str("OK\n"),
        Err(e) => writeln!(t, "ERR put: {}", e.message()),
    }
    .map_err(|fmt::Error| Stalled)
}

fn transfer<F: Files, P: Out>(
    arg: PutArg,
    open: Option<ProjectId>,
    files: &mut F,
    port: &mut P,
    due: Due,
) -> Result<(), PutFail> {
    let id = arg.to;
    if arg.size > PUT_MAX_BYTES {
        return Err(PutFail::TooBig);
    }
    if arg.size < MIN_FILE {
        return Err(PutFail::File(FileError::Truncated));
    }
    if open == Some(id) {
        return Err(PutFail::ProjectOpen);
    }
    due.arm(files, port, 0)?;
    Card::new()
        .run(files, |s, r| session(s, r, id, arg, port, due))
        .and_then(|o| o.result)
}

/// On the mounted card: the directory, `READY`, then the data. After
/// `READY` any failure drains the host before the `ERR`.
fn session<F: Files, P: Out>(
    s: &mut F,
    r: &Ready,
    id: ProjectId,
    arg: PutArg,
    port: &mut P,
    due: Due,
) -> Result<(), PutFail> {
    for d in [Dir::Chimera, Dir::Projects] {
        due.arm(s, port, 0)?;
        s.make_dir(r.volume(), d)?;
    }
    port.fresh();
    port.put(READY.as_bytes())
        .map_err(|Stalled| PutFail::Stalled)?;
    let mut src = Incoming::new(arg, port, due);
    let saved = save(s, r, id, &mut src);
    if saved.is_err() {
        let start = port.now();
        while port.now().since(start) < DRAIN_MS && port.recv(QUIET_MS).is_some() {}
    }
    saved
}

/// The header read, then the file saved as the unit saves it while the
/// project's own check reads it as it comes: a file the check refuses
/// ends before its trailer, a torn side a load never takes. Then the
/// headers, as the project list reads them: `OK` only if they show the
/// side just written as the one that loads.
#[inline(never)]
fn save<F: Files, P: Out>(
    s: &mut F,
    r: &Ready,
    id: ProjectId,
    src: &mut Incoming<'_, P>,
) -> Result<(), PutFail> {
    let file = project_file(id);
    let mut framer = Framer::new(src.size).map_err(PutFail::File)?;
    let mut check = ProjectCheck::default();
    let mut head = [0; HEADER_LEN];
    for b in &mut head {
        *b = src.next()?;
    }
    let h = peek_header(&head).map_err(PutFail::File)?;
    if h.kind != ProjectCheck::KIND {
        return Err(PutFail::File(FileError::WrongKind));
    }
    framer
        .push(&head, &mut |e: Event<'_>| check.event(e))
        .map_err(PutFail::File)?;
    let (side, generation) =
        save_target(s, r, file, &mut ProjectCheck::default()).map_err(|e| match e {
            SaveError::Store(e) => PutFail::Store(e),
            SaveError::File(e) => PutFail::File(e),
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
    judge(s, r, id, generation)
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

/// The host's data: base64 lines, decoded a line at a time, the stall
/// measured from the last whole block handed on.
struct Incoming<'p, P: Out> {
    port: &'p mut P,
    due: Due,
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

    /// The next data line, decoded.
    fn fill(&mut self) -> Result<(), PutFail> {
        let mut n = 0;
        loop {
            if self.due.left_ms(self.port) <= VERIFY_MS + SD_MARGIN_MS {
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
        let text = self.line.get(..n).unwrap_or(&[]);
        if text == ABORT {
            return Err(PutFail::Aborted);
        }
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
        if len % 3 != 0 && end != self.size {
            // Padding ends the file, not a line.
            return Err(PutFail::BadData);
        }
        (self.at, self.len) = (0, len);
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

/// `ls`'s whole answer: one `NAME size` line a file in PROJECTS, then
/// `OK`, or `ERR ls: <why>`. A write that stalls ends the listing's text,
/// not the card operation.
#[inline(never)]
pub fn ls<F: Files>(f: &mut F, out: &mut impl Out) -> Result<(), Stalled> {
    let mut t = super::Text(out);
    let mut wrote = Ok(());
    let listed = f.mount().and_then(|vol| {
        t.0.fresh();
        f.list(vol, Dir::Projects, &mut |name, len| {
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
