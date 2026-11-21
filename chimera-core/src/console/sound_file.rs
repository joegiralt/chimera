//! `rm` and `get`: a library Sound's pair over the console (library spec
//! § Audio and card rules), or a controller map's (ADR 0226); `rm` also a
//! tuning file (ADR 0240, 0259). Only `/CHIMERA/SOUNDS/S#######`,
//! `/CHIMERA/MAPS/<stem>` and, for `rm`, `/CHIMERA/TUNINGS/<stem>.SCL|KBM`
//! parse, so neither verb names any other file on the card.

use core::fmt::{self, Write};
use core::ops::ControlFlow;

use chimera_hal::store::{CHUNK, FileName, Files, ReadSink, Store, StoreError};

use super::put::{SOUNDS, map_pair, numbered, tuning};
use super::{Arg, Out, Stalled, Words};
use crate::project::PartCheck;
use crate::storage::{
    AbFile, Card, CardFault, Check, Crc32, FileError, FileKind, LibId, Pick, Ready, Side,
    check_file, check_frame, delete_ab, delete_order, library_file, pick,
};

/// `/CHIMERA/SOUNDS/S0000012` or `/CHIMERA/MAPS/FADE3C04`, any case: the
/// pair, no side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairArg {
    Sound(LibId),
    Map(AbFile),
}

impl Arg for PairArg {
    const USAGE: &'static str = "/CHIMERA/SOUNDS/S####### or /CHIMERA/MAPS/<stem>";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let w = words.next()?;
        let got = match numbered(w, SOUNDS, b'S') {
            Some(n) => PairArg::Sound(LibId::new(n)?),
            None => PairArg::Map(map_pair(w)?),
        };
        words.next().is_none().then_some(got)
    }
}

/// `rm`'s argument: a pair, or a tuning file `put` wrote (#906).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RmArg {
    Pair(PairArg),
    Tuning(FileName),
}

impl Arg for RmArg {
    const USAGE: &'static str =
        "/CHIMERA/SOUNDS/S#######, /CHIMERA/MAPS/<stem> or /CHIMERA/TUNINGS/<stem>.SCL|KBM";
    fn parse(words: Words<'_>) -> Option<Self> {
        let mut w = words;
        match w.next().and_then(tuning) {
            Some(f) => w.next().is_none().then_some(RmArg::Tuning(f)),
            None => PairArg::parse(words).map(RmArg::Pair),
        }
    }
}

impl PairArg {
    fn file(self) -> AbFile {
        match self {
            PairArg::Sound(id) => library_file(id),
            PairArg::Map(f) => f,
        }
    }
}

/// Whether either side of `file` is on the card, whole or not.
pub(super) fn exists<S: Store>(s: &mut S, r: &Ready, file: AbFile) -> Result<bool, StoreError> {
    struct Begun;
    impl ReadSink for Begun {
        fn begin(&mut self, _: u32) -> ControlFlow<()> {
            ControlFlow::Break(())
        }
        fn chunk(&mut self, _: &[u8]) -> ControlFlow<()> {
            ControlFlow::Break(())
        }
    }
    for side in [Side::A, Side::B] {
        match s.read(r.volume(), file.side(side), &mut Begun) {
            // A broken chain is a side a cut left: there all the same.
            Ok(()) | Err(StoreError::Corrupt) => return Ok(true),
            Err(StoreError::NotFound) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}

/// Why `rm` or `get` failed: `ERR <verb>: <message>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fail {
    Factory,
    NotFound,
    File(FileError),
    Store(StoreError),
}

impl Fail {
    fn message(self) -> &'static str {
        match self {
            Fail::Factory => "factory",
            Fail::NotFound => "not found",
            Fail::File(e) => e.message(),
            Fail::Store(e) => e.message(),
        }
    }
}

impl From<StoreError> for Fail {
    fn from(e: StoreError) -> Self {
        Fail::Store(e)
    }
}

impl CardFault for Fail {
    fn store_error(&self) -> Option<StoreError> {
        match *self {
            Fail::Store(e) => Some(e),
            _ => None,
        }
    }
}

/// `rm`'s whole answer: the pair deleted in ADR 0045's order, torn sides
/// too, then `OK`. A factory id refuses before the card is touched. A map's
/// pair goes in the same order, judged by its framing, and the UI's layers
/// read again.
#[inline(never)]
pub fn rm<F: Files>(arg: RmArg, f: &mut F, out: &mut impl Out) -> Result<(), Stalled> {
    let r = match arg {
        RmArg::Pair(PairArg::Sound(id)) if id.is_factory() => Err(Fail::Factory),
        _ => Card::new()
            .run(f, |s, r| {
                let arg = match arg {
                    RmArg::Pair(a) => a,
                    // The Parts on its `TUNEn` read again: a file gone
                    // plays 12-TET and says NOT FOUND.
                    RmArg::Tuning(t) => {
                        return match s.delete(r.volume(), t) {
                            Ok(()) => {
                                crate::tuning::file_put();
                                Ok(())
                            }
                            Err(StoreError::NotFound) => Err(Fail::NotFound),
                            Err(e) => Err(e.into()),
                        };
                    }
                };
                let file = arg.file();
                match (exists(s, r, file)?, arg) {
                    (false, _) => Err(Fail::NotFound),
                    (true, PairArg::Sound(_)) => Ok(delete_ab(s, r, file, &mut PartCheck::new())?),
                    (true, PairArg::Map(_)) => {
                        let st =
                            |s: &mut F, sd| check_frame(s, r, file.side(sd), FileKind::Controller);
                        let (a, b) = (st(s, Side::A)?, st(s, Side::B)?);
                        for side in delete_order(a, b) {
                            match s.delete(r.volume(), file.side(side)) {
                                Ok(()) | Err(StoreError::NotFound) => {}
                                Err(e) => return Err(e.into()),
                            }
                        }
                        crate::midi_map::map_put();
                        Ok(())
                    }
                }
            })
            .and_then(|o| o.result),
    };
    out.fresh();
    let mut t = super::Text(out);
    match r {
        Ok(()) => t.write_str("OK\n"),
        Err(e) => writeln!(t, "ERR rm: {}", e.message()),
    }
    .map_err(|fmt::Error| Stalled)
}

/// Base64 bytes a `get` line carries: 64 characters.
const GET_LINE: usize = 48;

static ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// A side read to the port: `GET <len>`, its base64 lines, their CRC.
struct Sent<'a, O: Out> {
    out: &'a mut O,
    line: [u8; GET_LINE],
    fill: usize,
    crc: Crc32,
    stalled: bool,
}

impl<O: Out> Sent<'_, O> {
    fn put(&mut self, b: &[u8]) -> ControlFlow<()> {
        match self.out.put(b) {
            Ok(()) => ControlFlow::Continue(()),
            Err(Stalled) => {
                self.stalled = true;
                ControlFlow::Break(())
            }
        }
    }

    /// The bytes held as one line, padded if short.
    fn flush(&mut self) -> ControlFlow<()> {
        let mut text = [0u8; GET_LINE / 3 * 4 + 1];
        let mut n = 0;
        for c in self.line[..self.fill].chunks(3) {
            let v = c
                .iter()
                .enumerate()
                .fold(0u32, |v, (i, &b)| v | u32::from(b) << (16 - 8 * i));
            for i in 0..4 {
                text[n + i] = match i <= c.len() {
                    true => ALPHABET[(v >> (18 - 6 * i) & 63) as usize],
                    false => b'=',
                };
            }
            n += 4;
        }
        text[n] = b'\n';
        self.fill = 0;
        self.put(&text[..=n])
    }
}

impl<O: Out> ReadSink for Sent<'_, O> {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        self.out.fresh();
        match writeln!(super::Text(&mut *self.out), "GET {len}") {
            Ok(()) => ControlFlow::Continue(()),
            Err(fmt::Error) => {
                self.stalled = true;
                ControlFlow::Break(())
            }
        }
    }

    fn chunk(&mut self, mut bytes: &[u8]) -> ControlFlow<()> {
        const _: () = assert!(CHUNK >= GET_LINE);
        self.crc.update(bytes);
        while !bytes.is_empty() {
            let take = (GET_LINE - self.fill).min(bytes.len());
            self.line[self.fill..self.fill + take].copy_from_slice(&bytes[..take]);
            (self.fill, bytes) = (self.fill + take, &bytes[take..]);
            if self.fill == GET_LINE {
                self.flush()?;
            }
        }
        ControlFlow::Continue(())
    }
}

/// `get`'s whole answer: the side a load picks, as `GET <len>`, base64
/// lines of 48 bytes, `CRC <crc32>`, then `OK`. A pair a load refuses
/// answers its `FileError`, both sides torn included.
#[inline(never)]
pub fn get<F: Files, P: Out>(arg: PairArg, f: &mut F, out: &mut P) -> Result<(), Stalled> {
    let file = arg.file();
    let mut sent = Sent {
        out,
        line: [0; GET_LINE],
        fill: 0,
        crc: Crc32::new(),
        stalled: false,
    };
    let r = Card::new()
        .run(f, |s, r| {
            let mut c = PartCheck::new();
            let mut side = |sd| match arg {
                PairArg::Sound(_) => {
                    c.side(sd);
                    check_file(s, r, file.side(sd), &mut c)
                }
                // A map's own rows are judged as it loads (bank 2's): here
                // its framing.
                PairArg::Map(_) => check_frame(s, r, file.side(sd), FileKind::Controller),
            };
            let (a, b) = (side(Side::A)?, side(Side::B)?);
            let sd = match pick(a, b) {
                Pick::Load(sd) => sd,
                Pick::Refuse(_, e) => return Err(Fail::File(e)),
                Pick::Missing => return Err(Fail::NotFound),
            };
            Ok(s.read(r.volume(), file.side(sd), &mut sent)?)
        })
        .and_then(|o| o.result);
    if sent.stalled {
        return Err(Stalled);
    }
    let tail = match (&r, sent.fill) {
        (Ok(()), 1..) => sent.flush(),
        _ => ControlFlow::Continue(()),
    };
    let crc = sent.crc.finish();
    let out = sent.out;
    if tail.is_break() {
        return Err(Stalled);
    }
    out.fresh();
    let mut t = super::Text(out);
    match r {
        Ok(()) => writeln!(t, "CRC {crc:08x}").and_then(|()| t.write_str("OK\n")),
        Err(e) => writeln!(t, "ERR get: {}", e.message()),
    }
    .map_err(|fmt::Error| Stalled)
}
