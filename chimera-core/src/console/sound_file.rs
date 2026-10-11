//! `rm` and `get`: a library Sound's pair over the console (library spec
//! § Audio and card rules). Only `/CHIMERA/SOUNDS/S#######` parses, so
//! neither verb names any other file on the card.

use core::fmt::{self, Write};
use core::ops::ControlFlow;

use chimera_hal::store::{CHUNK, Files, ReadSink, Store, StoreError};

use super::put::{SOUNDS, numbered};
use super::{Arg, Out, Stalled, Words};
use crate::storage::{
    AbFile, Card, CardFault, Check, Crc32, FileError, LibId, Pick, Ready, Side, SoundCheck,
    check_file, delete_ab, library_file, pick,
};

/// `/CHIMERA/SOUNDS/S0000012`, any case: the pair, no side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundArg(pub LibId);

impl Arg for SoundArg {
    const USAGE: &'static str = "/CHIMERA/SOUNDS/S#######";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let id = LibId::new(numbered(words.next()?, SOUNDS, b'S')?)?;
        words.next().is_none().then_some(SoundArg(id))
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
/// too, then `OK`. A factory id refuses before the card is touched.
#[inline(never)]
pub fn rm<F: Files>(arg: SoundArg, f: &mut F, out: &mut impl Out) -> Result<(), Stalled> {
    let file = library_file(arg.0);
    let r = match arg.0.is_factory() {
        true => Err(Fail::Factory),
        false => Card::new()
            .run(f, |s, r| match exists(s, r, file)? {
                false => Err(Fail::NotFound),
                true => Ok(delete_ab(s, r, file, &mut SoundCheck::new())?),
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
pub fn get<F: Files, P: Out>(arg: SoundArg, f: &mut F, out: &mut P) -> Result<(), Stalled> {
    let file = library_file(arg.0);
    let mut sent = Sent {
        out,
        line: [0; GET_LINE],
        fill: 0,
        crc: Crc32::new(),
        stalled: false,
    };
    let r = Card::new()
        .run(f, |s, r| {
            let mut c = SoundCheck::new();
            let mut side = |sd| {
                c.side(sd);
                check_file(s, r, file.side(sd), &mut c)
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
