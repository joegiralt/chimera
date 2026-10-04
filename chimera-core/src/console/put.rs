//! `ls` and `put`: files onto a card that can't leave the unit. `put`
//! streams base64 lines into a temporary file, checks its size and CRC-32,
//! then renames it over the target; any failure deletes it. Every card
//! operation runs under what is left of `PUT_DEADLINE_MS`.

use core::fmt::{self, Write};

use chimera_hal::Ms;
use chimera_hal::store::{ByteSink, CHUNK, Dir, FileName, Files, StoreError, VolumeId};

use super::{Arg, Out, Stalled, Words};
use crate::storage::Crc32;

/// The largest file `put` takes: a project is about 130 KB.
pub const PUT_MAX_BYTES: u32 = 1 << 20;
/// Free space a `put` must leave behind, beside its file.
pub const PUT_ROOM_BYTES: u64 = 1 << 20;
/// The whole `put`, from its line to its `OK`, card work included.
pub const PUT_DEADLINE_MS: u32 = 30_000;
/// The host silent this long mid-transfer ends the `put`: under the card's
/// 600 ms idle limit, which the wait for the next block counts against.
pub const PUT_STALL_MS: u32 = 500;
/// Base64 characters a data line holds at most: 48 bytes.
pub const PUT_LINE: usize = 64;
/// The line that ends a transfer early.
pub const ABORT: &[u8] = b"abort";
/// What the unit says once it will take the data.
pub const READY: &str = "READY\n";
/// Kept back from the rename for deleting the temporary file…
const CLEANUP_MS: u32 = 3_000;
/// …and from the write and its wait for data, for the rename and that.
const WRITE_RESERVE_MS: u32 = 2 * CLEANUP_MS;
/// After a failure the rest of the host's data is read and dropped until it
/// goes quiet this long, or for `DRAIN_MS` at most, before the `ERR`.
const QUIET_MS: u32 = 100;
const DRAIN_MS: u32 = 5_000;

const _: () = assert!(PUT_LINE.is_multiple_of(4));
const _: () = assert!(PUT_LINE <= super::MAX_LINE);

/// The directories `put` writes: SYSTEM and the bench files are out of reach.
pub const PUT_DIRS: [Dir; 2] = [Dir::Projects, Dir::Sounds];

/// The temporary file in `dir`. `P#######` and `S#######` lists never show it.
pub fn temp_file(dir: Dir) -> FileName {
    FileName::new(dir, b"PUT", b"TMP").expect("an 8.3 name")
}

/// A directory's path on the card.
pub fn dir_path(d: Dir) -> &'static str {
    match d {
        Dir::Chimera => "/CHIMERA",
        Dir::Projects => "/CHIMERA/PROJECTS",
        Dir::Sounds => "/CHIMERA/SOUNDS",
        Dir::Bench => "/CHIMERA/BENCH",
    }
}

const DIRS: [Dir; 4] = [Dir::Chimera, Dir::Projects, Dir::Sounds, Dir::Bench];

/// `/CHIMERA…` in any case, a trailing `/` allowed.
fn parse_dir(p: &[u8]) -> Option<Dir> {
    let p = p.strip_suffix(b"/").unwrap_or(p);
    DIRS.into_iter()
        .find(|d| p.eq_ignore_ascii_case(dir_path(*d).as_bytes()))
}

/// `<dir>/<STEM>[.<EXT>]` in any case. Only 8.3 names of A-Z and 0-9
/// parse, so `..`, `.` and a second `/` never do.
fn parse_file(p: &[u8]) -> Option<FileName> {
    let slash = p.iter().rposition(|&b| b == b'/')?;
    let (dir, name) = (parse_dir(p.get(..slash)?)?, p.get(slash + 1..)?);
    let (stem, ext) = match name.iter().position(|&b| b == b'.') {
        Some(i) => (name.get(..i)?, name.get(i + 1..)?),
        None => (name, &[][..]),
    };
    let mut up = [0u8; 11];
    let (s, e) = up.split_at_mut(8);
    let s = s.get_mut(..stem.len())?;
    let e = e.get_mut(..ext.len())?;
    s.copy_from_slice(stem);
    e.copy_from_slice(ext);
    s.make_ascii_uppercase();
    e.make_ascii_uppercase();
    FileName::new(dir, s, e)
}

/// `ls`'s argument: a directory under `/CHIMERA`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LsArg(pub Dir);

impl Arg for LsArg {
    const USAGE: &'static str = "/CHIMERA, /CHIMERA/PROJECTS, /CHIMERA/SOUNDS or /CHIMERA/BENCH";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let d = parse_dir(words.next()?)?;
        words.next().is_none().then_some(LsArg(d))
    }
}

/// `put`'s argument: where, how many bytes and their CRC-32/ISO-HDLC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PutArg {
    pub to: FileName,
    pub size: u32,
    pub crc: u32,
}

impl Arg for PutArg {
    const USAGE: &'static str =
        "/CHIMERA/PROJECTS/<8.3> or /CHIMERA/SOUNDS/<8.3>, its size and CRC-32 in hex";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let to = parse_file(words.next()?)?;
        let size = core::str::from_utf8(words.next()?).ok()?.parse().ok()?;
        let crc = words.next()?;
        if crc.len() > 8 {
            return None;
        }
        let crc = u32::from_str_radix(core::str::from_utf8(crc).ok()?, 16).ok()?;
        let ok = PUT_DIRS.contains(&to.dir()) && to != temp_file(to.dir());
        (ok && words.next().is_none()).then_some(PutArg { to, size, crc })
    }
}

/// Why a `put` failed: `ERR put: <message>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutFail {
    TooBig,
    NoRoom,
    Timeout,
    /// No byte for `PUT_STALL_MS`: a short transfer ends here.
    Stalled,
    Aborted,
    BadData,
    TooLong,
    BadCrc,
    Store(StoreError),
}

impl PutFail {
    pub fn message(self) -> &'static str {
        match self {
            PutFail::TooBig => "over 1 MiB",
            PutFail::NoRoom => "would leave under 1 MiB free",
            PutFail::Timeout => "timeout",
            PutFail::Stalled => "no data for 500 ms",
            PutFail::Aborted => "aborted",
            PutFail::BadData => "bad data line",
            PutFail::TooLong => "more data than its size",
            PutFail::BadCrc => "CRC mismatch",
            PutFail::Store(e) => e.message(),
        }
    }
}

impl From<StoreError> for PutFail {
    fn from(e: StoreError) -> Self {
        PutFail::Store(e)
    }
}

/// The one deadline, on the link's clock.
struct Due(Ms);

impl Due {
    fn left_ms(&self, link: &impl Out) -> u32 {
        PUT_DEADLINE_MS.saturating_sub(link.now().since(self.0))
    }

    /// Caps the next card operation at what is left less `reserve_ms`:
    /// `Timeout` if nothing is.
    fn arm<F: Files + ?Sized>(
        &self,
        files: &mut F,
        link: &impl Out,
        reserve_ms: u32,
    ) -> Result<(), PutFail> {
        let left = self.left_ms(link).saturating_sub(reserve_ms);
        if left == 0 {
            return Err(PutFail::Timeout);
        }
        files.cap(Some(left));
        Ok(())
    }

    /// `f`, one card operation, under `arm(reserve_ms)`.
    fn op<F: Files + ?Sized, T>(
        &self,
        files: &mut F,
        link: &impl Out,
        reserve_ms: u32,
        f: impl FnOnce(&mut F) -> Result<T, StoreError>,
    ) -> Result<T, PutFail> {
        self.arm(files, link, reserve_ms)?;
        f(files).map_err(PutFail::Store)
    }
}

/// `put`'s whole answer: `READY` once the card has room, then `OK`, or
/// `ERR put: <why>` with no temporary file left.
pub fn put<F: Files + ?Sized, P: Out>(
    arg: PutArg,
    files: &mut F,
    port: &mut P,
) -> Result<(), Stalled> {
    let due = Due(port.now());
    let r = transfer(arg, files, port, &due);
    files.cap(None);
    let mut t = super::Text(port);
    match r {
        Ok(()) => t.write_str("OK\n"),
        Err(e) => writeln!(t, "ERR put: {}", e.message()),
    }
    .map_err(|fmt::Error| Stalled)
}

fn transfer<F: Files + ?Sized, P: Out>(
    arg: PutArg,
    files: &mut F,
    port: &mut P,
    due: &Due,
) -> Result<(), PutFail> {
    if arg.size > PUT_MAX_BYTES {
        return Err(PutFail::TooBig);
    }
    let dir = arg.to.dir();
    let vol = due.op(files, port, 0, |f| f.mount())?;
    due.op(files, port, 0, |f| f.make_dir(vol, Dir::Chimera))?;
    due.op(files, port, 0, |f| f.make_dir(vol, dir))?;
    let free = due.op(files, port, 0, |f| f.free_bytes(vol))?;
    if free < u64::from(arg.size) + PUT_ROOM_BYTES {
        return Err(PutFail::NoRoom);
    }
    port.fresh();
    port.put(READY.as_bytes())
        .map_err(|Stalled| PutFail::Stalled)?;
    let temp = temp_file(dir);
    let written = write_temp(arg, files, port, due, vol, temp)
        .and_then(|()| due.op(files, port, CLEANUP_MS, |f| f.rename(vol, temp, arg.to)));
    if written.is_err() {
        // A failed write re-inited the card: mount it again first.
        let _ = due.op(files, port, 0, |f| {
            let vol = f.mount()?;
            match f.delete(vol, temp) {
                Err(StoreError::NotFound) => Ok(()),
                r => r,
            }
        });
        let start = port.now();
        while port.recv(QUIET_MS).is_some() && port.now().since(start) < DRAIN_MS {}
    }
    port.fresh();
    written
}

/// The data into `temp`, under the deadline less `CLEANUP_MS`.
fn write_temp<F: Files + ?Sized, P: Out>(
    arg: PutArg,
    files: &mut F,
    port: &mut P,
    due: &Due,
    vol: VolumeId,
    temp: FileName,
) -> Result<(), PutFail> {
    due.arm(files, port, WRITE_RESERVE_MS)?;
    let mut why = None;
    let wrote = files.write(vol, temp, &mut |sink| {
        receive(arg, port, sink, due).map_err(|e| match e {
            PutFail::Store(e) => e,
            e => {
                why = Some(e);
                StoreError::Io
            }
        })
    });
    match (why, wrote) {
        (Some(e), _) => Err(e),
        (None, Ok(n)) if n == arg.size => Ok(()),
        (None, Ok(_)) => Err(PutFail::Store(StoreError::Io)),
        (None, Err(e)) => Err(PutFail::Store(e)),
    }
}

/// Base64 lines from the host into `sink`, a block at a time so the wait
/// for the next block never sits inside the card's: exactly `size` bytes
/// whose CRC is `crc`.
fn receive(
    arg: PutArg,
    link: &mut impl Out,
    sink: &mut dyn ByteSink,
    due: &Due,
) -> Result<(), PutFail> {
    let mut block = [0u8; CHUNK];
    let (mut fill, mut got) = (0usize, 0u32);
    let mut crc = Crc32::new();
    let mut line = [0u8; PUT_LINE];
    let mut n = 0;
    while got < arg.size {
        if due.left_ms(link) <= WRITE_RESERVE_MS {
            return Err(PutFail::Timeout);
        }
        let b = link.recv(PUT_STALL_MS).ok_or(PutFail::Stalled)?;
        if b != b'\n' && b != b'\r' {
            *line.get_mut(n).ok_or(PutFail::BadData)? = b;
            n += 1;
            continue;
        }
        let text = line.get(..n).unwrap_or(&[]);
        n = 0;
        if text == ABORT {
            return Err(PutFail::Aborted);
        }
        let (quads, rest) = text.as_chunks::<4>();
        if !rest.is_empty() {
            return Err(PutFail::BadData);
        }
        let last = quads.len().saturating_sub(1);
        for (i, q) in quads.iter().enumerate() {
            let (bytes, len) = quad(q, i == last).ok_or(PutFail::BadData)?;
            for &x in bytes.get(..len).unwrap_or(&[]) {
                if got == arg.size {
                    return Err(PutFail::TooLong);
                }
                *block.get_mut(fill).ok_or(PutFail::BadData)? = x;
                (fill, got) = (fill + 1, got + 1);
                if fill == CHUNK {
                    crc.update(&block);
                    sink.put(&block)?;
                    fill = 0;
                }
            }
            if len < 3 && got != arg.size {
                // Padding ends the data.
                return Err(PutFail::BadData);
            }
        }
    }
    let tail = block.get(..fill).unwrap_or(&[]);
    crc.update(tail);
    sink.put(tail)?;
    if crc.finish() == arg.crc {
        Ok(())
    } else {
        Err(PutFail::BadCrc)
    }
}

/// One base64 quad: its bytes and how many. Padding only in the last.
fn quad(q: &[u8; 4], last: bool) -> Option<([u8; 3], usize)> {
    let v = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
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
        let x = if i >= 4 - pad { 0 } else { v(c)? };
        n = n << 6 | u32::from(x);
    }
    let [_, a, b, c] = n.to_be_bytes();
    Some(([a, b, c], 3 - pad))
}

/// A name as the card shows it: `STEM.EXT`, or `STEM`.
pub struct Shown(pub FileName);

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

/// `ls`'s body on `vol`: one `NAME size` line a file. A write that
/// stalls is kept and returned once the listing ends.
pub fn write_ls<F: Files + ?Sized>(
    files: &mut F,
    vol: VolumeId,
    dir: Dir,
    t: &mut impl Write,
) -> Result<fmt::Result, StoreError> {
    let mut wrote = Ok(());
    files.list(vol, dir, &mut |name, len| {
        if wrote.is_ok() {
            wrote = writeln!(t, "{} {len}", Shown(name));
        }
    })?;
    Ok(wrote)
}
