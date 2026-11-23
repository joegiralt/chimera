// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SETTINGS › TUNING's card side in bank 1 (ADR 0259, under ADR 0251:
//! ORBIT's bank 2 is full): a TUNING turn past MTS through the card's
//! Scala files in the order of the names they show, the file a Part names
//! found by that name, and what the last read file shows. `ui::tuning`,
//! behind the door, calls these on the card pass.
//!
//! Each entry bank 2 calls is `#[inline(never)]`: inlined, it would land
//! in the caller's bank.
//!
//! A file copied with a long name shows it (`Store::list_long`), less
//! `.SCL`; one without shows its stem. A Part keeps the stem and the shown
//! name's id (`ScalaName`): the id finds the file again when a copy gave
//! it another short name.

// Without `tuning` no file is read: the reader's side is dead code.
#![cfg_attr(not(feature = "tuning"), allow(dead_code))]

use core::ops::ControlFlow;
use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use chimera_hal::store::{Dir, FileName, Store};

use crate::part::{TUNING_MTS, TUNING_SCALA};
use crate::project::PartId;
use crate::scala_name::{LONG_MAX, name_id, shown};
use crate::storage::{Card, Ready};
use crate::tuning::ScalaName;
use crate::ui::UiState;

/// A description's room on the focus band.
pub const DESC_LEN: usize = 30;

/// A file's place in the walk: the name it shows, zero padded, then its
/// stem (two files can show one name).
type Key = ([u8; LONG_MAX], [u8; 8]);

/// Where a TUNING turn stands: a built-in choice, a file, or before the
/// first file.
#[derive(Clone, Copy)]
enum Pos {
    Built(u8),
    File(Option<(Key, ScalaName)>),
}

/// Part `p`'s TUNING `d` detents on: through the built-in choices, then
/// the card's files, stopping at each end. A card pass per turn, a
/// folder read per file detent: nothing listed is kept.
#[inline(never)]
pub(super) fn turn(ui: &mut UiState, card: &mut Card, s: &mut dyn Store, p: PartId, d: i8) {
    let part = ui.project.part(p);
    let from = match (part.mix.tuning, part.scala()) {
        (TUNING_SCALA, Some(n)) => Pos::File(here(card, s, n)),
        (TUNING_SCALA, None) => Pos::File(None),
        (t, _) => Pos::Built(t.min(TUNING_MTS)),
    };
    let mut at = from;
    let (mut moved, mut capped) = (false, false);
    for _ in 0..d.unsigned_abs() {
        let next = match (at, d > 0) {
            (Pos::Built(b), true) if b < TUNING_MTS => Pos::Built(b + 1),
            (Pos::Built(_), true) => match near(card, s, None, true) {
                (Some(f), _) => Pos::File(Some(f)),
                (None, more) => {
                    capped = more;
                    break;
                }
            },
            (Pos::Built(b), false) => Pos::Built(b.saturating_sub(1)),
            (Pos::File(c), up) => match near(card, s, c.map(|c| c.0), up) {
                (Some(f), _) => Pos::File(Some(f)),
                (None, more) if up => {
                    capped = more;
                    break;
                }
                (None, _) => Pos::Built(TUNING_MTS),
            },
        };
        (at, moved) = (next, true);
    }
    if capped {
        // The end of the files reached, not of the folder.
        ui.toast.show(crate::ui::busy::Toast {
            text: crate::project::Line::new("FIRST 128 FILES ONLY"),
            ms: crate::ui::busy::Toast::ERROR_MS,
        });
    }
    if !moved {
        return;
    }
    let (t, n) = match at {
        Pos::Built(b) => (b, None),
        Pos::File(f) => (TUNING_SCALA, f.map(|f| f.1)),
    };
    ui.project.edit_part(p).mix.tuning = t;
    ui.project.set_scala(p, n);
}

/// The most `.SCL` files a folder read reaches (stock's cap): a detent
/// costs a bounded read, however many files the card holds (ADR 0259).
pub const MAX_FILES: usize = 128;
/// The most files of any kind a folder read passes.
const MAX_SEEN: usize = 2 * MAX_FILES;

/// The first `MAX_FILES` `.SCL` files in folder order, each with the name
/// it shows; `true` if the folder holds more than were reached.
pub fn scan_scales(
    s: &mut dyn Store,
    vol: chimera_hal::store::VolumeId,
    f: &mut dyn FnMut(&FileName, &[u8]),
) -> Result<bool, chimera_hal::store::StoreError> {
    let (mut scales, mut seen, mut more) = (0, 0, false);
    s.list_long(vol, Dir::Tunings, &mut |file, _, long| {
        let scale = file.ext() == b"SCL";
        if scales == MAX_FILES || seen == MAX_SEEN {
            more |= scale || seen == MAX_SEEN;
            return ControlFlow::Break(());
        }
        seen += 1;
        if scale {
            scales += 1;
            f(&file, shown(long, &file));
        }
        ControlFlow::Continue(())
    })?;
    Ok(more)
}

/// `scan_scales` on the card; `true` if files were left out. A card that
/// won't list lists nothing.
fn scales(card: &mut Card, s: &mut dyn Store, f: &mut dyn FnMut(&FileName, &[u8])) -> bool {
    card.run(s, |s, r| scan_scales(s, r.volume(), f))
        .ok()
        .and_then(|o| o.result.ok())
        .unwrap_or(false)
}

fn key(file: &FileName, name: &[u8]) -> Key {
    let mut k = ([0; LONG_MAX], [0; 8]);
    let n = name.len().min(LONG_MAX);
    k.0[..n].copy_from_slice(&name[..n]);
    k.1[..file.stem().len()].copy_from_slice(file.stem());
    k
}

/// How well file `f`, showing a name of id `id`, is the one `n` names:
/// 3 its stem and id (two long names can share an id: their first 32
/// characters alike), 2 the id alone (copied again: another `~N`), 1 the
/// stem alone (renamed, or a project from before long names), 0 not.
fn rank(n: ScalaName, f: &FileName, id: u32) -> u8 {
    let stem = f.stem() == n.as_bytes();
    match (n.id() != 0 && id == n.id(), stem) {
        (true, true) => 3,
        (true, false) => 2,
        (false, true) => 1,
        (false, false) => 0,
    }
}

/// The file `n` names, as the walk places it; `None` if the card hasn't
/// it (among the files reached).
fn here(card: &mut Card, s: &mut dyn Store, n: ScalaName) -> Option<(Key, ScalaName)> {
    // The name the active Part's file shows is kept: no folder read.
    let mut name = [0; LONG_MAX];
    let len = SHOWN.name(n, &mut name);
    if len > 0
        && let Some(f) = n.file(b"SCL")
    {
        return Some((key(&f, &name[..len]), n));
    }
    let mut found = None;
    let mut best = 0;
    scales(card, s, &mut |f, name| {
        let id = name_id(name);
        let r = rank(n, f, id);
        if r > best
            && let Some(m) = ScalaName::new(f.stem())
        {
            (best, found) = (r, Some((key(f, name), m.with_id(id))));
        }
    });
    found
}

/// The file nearest `c` in the walk, after it (`up`) or before it; `None`
/// for `c` is before the first. Also whether files were left out.
fn near(
    card: &mut Card,
    s: &mut dyn Store,
    c: Option<Key>,
    up: bool,
) -> (Option<(Key, ScalaName)>, bool) {
    let mut best: Option<(Key, ScalaName)> = None;
    let more = scales(card, s, &mut |f, name| {
        let k = key(f, name);
        let past = match c {
            None => up,
            Some(c) if up => k > c,
            Some(c) => k < c,
        };
        let nearer = match &best {
            None => true,
            Some(b) => (k < b.0) == up,
        };
        if past
            && nearer
            && let Some(m) = ScalaName::new(f.stem())
        {
            best = Some((k, m.with_id(name_id(name))));
        }
    });
    (best, more)
}

/// The files `n` plays: its `.SCL` (the best `rank`) and the `.KBM`
/// showing the same name (its stem's first), and the name the scale shows
/// into `name` (its length). Two bounded folder reads (ADR 0259).
#[inline(never)]
pub(super) fn find(
    s: &mut dyn Store,
    r: &Ready,
    n: ScalaName,
    name: &mut [u8; LONG_MAX],
) -> (Option<FileName>, Option<FileName>, usize) {
    let (mut scl, mut id, mut len, mut best) = (None::<FileName>, 0, 0, 0);
    let _ = scan_scales(s, r.volume(), &mut |f, shows| {
        let i = name_id(shows);
        let k = rank(n, f, i);
        if k > best {
            len = shows.len().min(LONG_MAX);
            name[..len].copy_from_slice(&shows[..len]);
            (scl, id, best) = (Some(*f), i, k);
        }
    });
    let Some(sf) = scl else {
        return (None, None, 0);
    };
    let (mut kbm, mut seen) = (None::<FileName>, 0);
    let _ = s.list_long(r.volume(), Dir::Tunings, &mut |f, _, long| {
        seen += 1;
        if f.ext() == b"KBM" && name_id(shown(long, &f)) == id {
            let same = f.stem() == sf.stem();
            if kbm.is_none() || same {
                kbm = Some(f);
            }
            if same {
                return ControlFlow::Break(());
            }
        }
        if seen == MAX_SEEN {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });
    (scl, kbm, len)
}

/// What the card pass last read, for the page (AXI, not the UI's D2): the
/// file a Part named, the name it shows and its description, upper case.
/// Atomics: the UI thread alone writes it; a host test's reads stay sound.
pub struct Shown {
    /// The stem's 8 bytes, as two words (no 64-bit atomics on the M7).
    stem: [AtomicU32; 2],
    id: AtomicU32,
    name: [AtomicU8; LONG_MAX],
    name_len: AtomicU8,
    desc: [AtomicU8; DESC_LEN],
    desc_len: AtomicU8,
}

pub static SHOWN: Shown = Shown {
    stem: [AtomicU32::new(0), AtomicU32::new(0)],
    id: AtomicU32::new(0),
    name: [const { AtomicU8::new(0) }; LONG_MAX],
    name_len: AtomicU8::new(0),
    desc: [const { AtomicU8::new(0) }; DESC_LEN],
    desc_len: AtomicU8::new(0),
};

fn words(n: ScalaName) -> [u32; 2] {
    let p = n.padded();
    [
        u32::from_le_bytes([p[0], p[1], p[2], p[3]]),
        u32::from_le_bytes([p[4], p[5], p[6], p[7]]),
    ]
}

fn put(to: &[AtomicU8], len: &AtomicU8, b: &[u8]) {
    let n = b.len().min(to.len());
    for (t, &c) in to.iter().zip(&b[..n]) {
        t.store(c, Ordering::Relaxed);
    }
    len.store(n as u8, Ordering::Relaxed);
}

fn get(from: &[AtomicU8], len: &AtomicU8, out: &mut [u8]) -> usize {
    let n = usize::from(len.load(Ordering::Relaxed)).min(out.len());
    for (o, f) in out[..n].iter_mut().zip(from) {
        *o = f.load(Ordering::Relaxed);
    }
    n
}

impl Shown {
    /// `n`'s file shows `name`; its description not read yet.
    #[inline(never)]
    pub(super) fn set_file(&self, n: ScalaName, name: &[u8]) {
        for (w, s) in self.stem.iter().zip(words(n)) {
            w.store(s, Ordering::Relaxed);
        }
        self.id.store(n.id(), Ordering::Relaxed);
        put(&self.name, &self.name_len, name);
        self.desc_len.store(0, Ordering::Relaxed);
    }

    /// The description of the file last set, upper case, unprintable
    /// bytes as spaces (here, in bank 1: bank 2 is full).
    #[inline(never)]
    pub(super) fn set_desc(&self, d: &[u8]) {
        let mut up = [0; DESC_LEN];
        let n = d.len().min(DESC_LEN);
        for (to, &c) in up.iter_mut().zip(&d[..n]) {
            *to = match c.to_ascii_uppercase() {
                c @ 0x20..=0x7e => c,
                _ => b' ',
            };
        }
        put(&self.desc, &self.desc_len, &up[..n]);
    }

    fn is(&self, n: ScalaName) -> bool {
        let [a, b] = words(n);
        self.stem[0].load(Ordering::Relaxed) == a
            && self.stem[1].load(Ordering::Relaxed) == b
            && self.id.load(Ordering::Relaxed) == n.id()
    }

    /// The name `n`'s file shows into `out` (its length), 0 if the last
    /// read was another's.
    #[inline(never)]
    pub fn name(&self, n: ScalaName, out: &mut [u8; LONG_MAX]) -> usize {
        if !self.is(n) {
            return 0;
        }
        get(&self.name, &self.name_len, out)
    }

    /// `n`'s description as the band's line; `None` if not read or blank.
    #[inline(never)]
    pub fn desc(&self, n: ScalaName) -> Option<crate::project::Line> {
        let mut d = [0; DESC_LEN];
        let k = if self.is(n) {
            get(&self.desc, &self.desc_len, &mut d)
        } else {
            0
        };
        (k > 0)
            .then(|| crate::project::Line::new(core::str::from_utf8(&d[..k]).unwrap_or_default()))
    }
}

/// UI frames a marquee holds still at each end (1.5 s at 20 fps).
pub const MARQUEE_HOLD: u32 = 30;
/// UI frames a marquee holds each character (4 a second).
pub const MARQUEE_STEP: u32 = 5;

/// Where a `w`-wide window over an `n`-character name starts at UI frame
/// `t`: still at the start, a character every `MARQUEE_STEP` to the end,
/// still there, then from the start again. 0 when the name fits.
pub fn marquee(n: usize, w: usize, t: u32) -> usize {
    let Some(steps) = n.checked_sub(w).filter(|&s| s > 0) else {
        return 0;
    };
    let steps = steps as u32;
    let t = t % (2 * MARQUEE_HOLD + steps * MARQUEE_STEP);
    (t.saturating_sub(MARQUEE_HOLD) / MARQUEE_STEP).min(steps) as usize
}
