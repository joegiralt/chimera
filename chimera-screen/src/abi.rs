// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The wasm exports: plain `extern "C"`, one mirror per engine and their
//! buffers. JS writes a batch's addresses and values through `text_ptr`
//! and `vals_ptr`, and reads the pixels and rectangle through `buf_ptr`
//! and `rect_ptr`. Every answer is a `Status`.
//!
//! | export | meaning |
//! |---|---|
//! | `engines_ptr() -> *const u8`, `engines_len() -> u32` | the engines' names, `\n` between, in `EngineType::PLAYABLE`'s order, then `MIXER` (the mixer's FX pages, `ed describe MIXER`'s numbering): an engine's number is its place here |
//! | `text_ptr() -> *mut u8` | `TEXT_CAP` bytes: a batch's addresses, `\n` between |
//! | `vals_ptr() -> *mut f32` | `VALS_CAP` stored values, one per address |
//! | `apply(engine: u8, n: u32, len: u32) -> i32` | the engine's mirror takes the batch's `n` values (`len` bytes of text): `Ok`, `Refused` (`refused()` says how many), or an error |
//! | `refused() -> u32`, `writes() -> u32` | the last `apply`'s refusals, and the values it wrote |
//! | `reset(engine: u8) -> i32` | the engine's mirror back to its INIT Sound; MIXER's to the project's default FX |
//! | `show(engine: u8, page: u16, region: u8, focus: i32, pad: u8, frame: u32) -> i32` | page `page` drawn (`focus` -1 none, else knob a-f as 0-5; `pad` 0 none, else 1-12) at UI frame `frame` (an animated glyph's phase; 0 when left out), or, for `Region::PadIcon`, pad `pad`'s icon alone; `NotShown` where the Sound doesn't show the page, and on MIXER past its shared FX pages |
//! | `buf_ptr() -> *const u16` | the 240x320 RGB565 pixels, row by row |
//! | `rect_ptr() -> *const u16` | `[x, y, w, h]` of the last `show`'s region |
//!
//! Every pointer is into the one `Shell`, built on the first call and never
//! dropped or moved: the module's instance owns it. Each buffer is a fixed
//! array inside it, so a pointer stays valid for the instance's life; JS
//! reads or writes it only between calls (wasm32 is one thread).

use crate::{Addr, Board, Fb, Mirror, PageNo, Region, Shown, Slot};
use chimera_core::console::Schema;
use chimera_core::dsp::drum::types::PadId;
use chimera_core::params::EngineType;
use core::cell::RefCell;

/// The longest batch text `apply` reads.
pub const TEXT_CAP: usize = 32 * 1024;
/// The most values a batch carries: DRUM's 672 described, and room.
pub const VALS_CAP: usize = 1024;

/// What every export answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Status {
    Ok = 0,
    /// Some of the batch didn't land (`refused()`); the rest did.
    Refused = 1,
    /// Not UTF-8, more than the buffers hold, or not `n` addresses.
    BadBatch = -1,
    NoSuchEngine = -2,
    /// The engine has no such page, or the Sound doesn't show it.
    NotShown = -3,
    /// The focus names no knob, or an empty cell.
    NoSuchCell = -4,
    NoSuchRegion = -5,
    /// The region has no rectangle on this page (a BigViz page's gauge).
    NoRegion = -6,
    /// A pad outside 1-12, or `PadIcon` with none.
    NoSuchPad = -7,
}

impl From<Status> for i32 {
    fn from(s: Status) -> i32 {
        s as i32
    }
}

struct Shell {
    /// `EngineType::PLAYABLE`'s mirrors, then the mixer's, each built on
    /// first use.
    mirrors: Mirrors,
    fb: Fb,
    text: Box<[u8; TEXT_CAP]>,
    vals: Box<[f32; VALS_CAP]>,
    rect: [u16; 4],
    names: String,
    refused: u32,
    writes: u32,
}

thread_local! {
    static SHELL: RefCell<Option<Shell>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut Shell) -> R) -> R {
    SHELL.with(|s| {
        let mut s = s.borrow_mut();
        let s = s.get_or_insert_with(|| Shell {
            mirrors: Default::default(),
            fb: Fb::new(),
            text: Box::new([0; TEXT_CAP]),
            vals: Box::new([0.0; VALS_CAP]),
            rect: [0; 4],
            names: names(),
            refused: 0,
            writes: 0,
        });
        f(s)
    })
}

/// `engines_ptr`'s list: the engines, then the mixer, by their `ed
/// describe` names.
fn names() -> String {
    let mut out = String::new();
    for (i, s) in BOARDS.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let _ = s.write(&mut out);
    }
    out
}

/// Each mirror's schema, in its number's place.
const BOARDS: [Schema; EngineType::PLAYABLE.len() + 1] = {
    let mut b = [Schema::Mixer; EngineType::PLAYABLE.len() + 1];
    let mut i = 0;
    while i < EngineType::PLAYABLE.len() {
        b[i] = Schema::Engine(EngineType::PLAYABLE[i]);
        i += 1;
    }
    b
};

type Mirrors = [Option<Mirror>; BOARDS.len()];

/// Mirror `n` as it starts: an engine's INIT Sound, or the mixer's
/// defaults.
fn fresh(n: u8) -> Result<Mirror, Status> {
    match BOARDS.get(usize::from(n)).ok_or(Status::NoSuchEngine)? {
        Schema::Engine(e) => {
            let mut m = Mirror::new();
            m.init(*e);
            Ok(m)
        }
        Schema::Mixer => Ok(Mirror::mixer()),
        // No mirror walks these: `BOARDS` never lists them.
        Schema::Matrix(_) | Schema::Settings | Schema::Map | Schema::Orbit => {
            Err(Status::NoSuchEngine)
        }
    }
}

/// Mirror `n`, as `fresh` the first time.
fn mirror(mirrors: &mut Mirrors, n: u8) -> Result<&mut Mirror, Status> {
    let m = mirrors
        .get_mut(usize::from(n))
        .ok_or(Status::NoSuchEngine)?;
    if m.is_none() {
        *m = Some(fresh(n)?);
    }
    m.as_mut().ok_or(Status::NoSuchEngine)
}

impl Shell {
    /// The batch: `n` addresses in the text's first `len` bytes, each with
    /// its value.
    fn batch(&self, n: u32, len: u32) -> Result<Vec<(Addr, f32)>, Status> {
        let n = usize::try_from(n).map_err(|_| Status::BadBatch)?;
        let text = self
            .text
            .get(..len as usize)
            .and_then(|b| str::from_utf8(b).ok())
            .ok_or(Status::BadBatch)?;
        let vals = self.vals.get(..n).ok_or(Status::BadBatch)?;
        let addrs: Vec<_> = if n == 0 {
            Vec::new()
        } else {
            text.split('\n').collect()
        };
        if addrs.len() != n {
            return Err(Status::BadBatch);
        }
        addrs
            .into_iter()
            .zip(vals.iter().copied())
            .map(|(a, v)| Addr::parse(a).map(|a| (a, v)).ok_or(Status::BadBatch))
            .collect()
    }
}

fn status(r: Result<Status, Status>) -> i32 {
    r.unwrap_or_else(|e| e).into()
}

// SAFETY (every `no_mangle` below): each export's name is its own in the
// module: no other symbol of the crate or its dependencies is named
// `engines_ptr`, `engines_len`, `text_ptr`, `vals_ptr`, `apply`,
// `refused`, `writes`, `reset`, `show`, `buf_ptr` or `rect_ptr`.

// SAFETY: see above; the pointer is the `Shell`'s `names`, never changed
// after it is built, and `engines_len` its length.
#[unsafe(no_mangle)]
pub extern "C" fn engines_ptr() -> *const u8 {
    with(|s| s.names.as_ptr())
}

// SAFETY: see above.
#[unsafe(no_mangle)]
pub extern "C" fn engines_len() -> u32 {
    with(|s| s.names.len() as u32)
}

// SAFETY: see above; `TEXT_CAP` bytes inside the `Shell`, valid for the
// instance's life.
#[unsafe(no_mangle)]
pub extern "C" fn text_ptr() -> *mut u8 {
    with(|s| s.text.as_mut_ptr())
}

// SAFETY: see above; `VALS_CAP` f32s inside the `Shell`, valid for the
// instance's life.
#[unsafe(no_mangle)]
pub extern "C" fn vals_ptr() -> *mut f32 {
    with(|s| s.vals.as_mut_ptr())
}

// SAFETY: see above.
#[unsafe(no_mangle)]
pub extern "C" fn apply(engine: u8, n: u32, len: u32) -> i32 {
    status(with(|s| {
        let batch = s.batch(n, len)?;
        let done = mirror(&mut s.mirrors, engine)?.apply(&batch);
        s.refused = done.refused.len() as u32;
        s.writes = done.writes as u32;
        Ok(if done.refused.is_empty() {
            Status::Ok
        } else {
            Status::Refused
        })
    }))
}

// SAFETY: see above.
#[unsafe(no_mangle)]
pub extern "C" fn refused() -> u32 {
    with(|s| s.refused)
}

// SAFETY: see above.
#[unsafe(no_mangle)]
pub extern "C" fn writes() -> u32 {
    with(|s| s.writes)
}

// SAFETY: see above.
#[unsafe(no_mangle)]
pub extern "C" fn reset(engine: u8) -> i32 {
    status(with(|s| {
        let m = mirror(&mut s.mirrors, engine)?;
        let schema = *BOARDS
            .get(usize::from(engine))
            .ok_or(Status::NoSuchEngine)?;
        match (m.board(), schema) {
            (Board::Sound, Schema::Engine(e)) => m.init(e),
            (Board::Mixer, Schema::Mixer) => *m = Mirror::mixer(),
            // A mirror on another board than its number's: never built.
            (Board::Sound | Board::Mixer, _) => return Err(Status::NoSuchEngine),
        }
        Ok(Status::Ok)
    }))
}

// SAFETY: see above.
#[unsafe(no_mangle)]
pub extern "C" fn show(engine: u8, page: u16, region: u8, focus: i32, pad: u8, frame: u32) -> i32 {
    status(with(|s| {
        let r = Region::from_u8(region).ok_or(Status::NoSuchRegion)?;
        let focus = match focus {
            -1 => None,
            n => Some(
                u8::try_from(n)
                    .ok()
                    .and_then(Slot::new)
                    .ok_or(Status::NoSuchCell)?,
            ),
        };
        let pad = match pad {
            0 => None,
            n => Some(PadId::new(n - 1).ok_or(Status::NoSuchPad)?),
        };
        let (m, fb) = (mirror(&mut s.mirrors, engine)?, &mut s.fb);
        let rect = if r == Region::PadIcon {
            m.pad_icon(pad.ok_or(Status::NoSuchPad)?, fb);
            crate::Rect::PAD_ICON
        } else {
            m.render(PageNo(page), Shown { focus, pad, frame }, fb)
                .ok_or(Status::NotShown)?;
            m.region(r).ok_or(Status::NoRegion)?
        };
        s.rect = [rect.x, rect.y, rect.w, rect.h];
        Ok(Status::Ok)
    }))
}

// SAFETY: see above; the `Shell`'s frame, valid for the instance's life.
#[unsafe(no_mangle)]
pub extern "C" fn buf_ptr() -> *const u16 {
    with(|s| s.fb.0.as_ptr())
}

// SAFETY: see above; the `Shell`'s four `u16`s, valid for the instance's
// life.
#[unsafe(no_mangle)]
pub extern "C" fn rect_ptr() -> *const u16 {
    with(|s| s.rect.as_ptr())
}
