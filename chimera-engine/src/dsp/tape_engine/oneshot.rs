// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! ONESHOT's slices (spec § 2 Machines): 16 exact, equal slices of the
//! window, as CHOMPI's TEMPO cuts a loop (ideas only, ADR 0093). Under
//! NOTE the key picks one (C4 is slice 1, wrapping every 16), under 1–16
//! the knob does.

use super::heads::{Dir, Window};

/// The note that plays slice 1 under SLICE NOTE: C4.
pub const SLICE_NOTE_BASE: u8 = 60;
/// SLICE's choices: OFF, NOTE, then the 16 slices.
pub const SLICE_CHOICES: u8 = 18;

/// SLICE's value, its code: 0 OFF, 1 NOTE, 2–17 slices 1–16. One byte,
/// as the snapshot has no room for more; a code past 17 can't be built.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct SliceCode(u8);

/// Shown as its code, as the raw byte was: the neutral Sound's text is a
/// format contract (`neutral_is_a_contract`).
impl core::fmt::Debug for SliceCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl SliceCode {
    pub const OFF: SliceCode = SliceCode(0);
    pub const NOTE: SliceCode = SliceCode(1);

    /// Code `c`; past the last, the last.
    pub const fn new(c: u8) -> SliceCode {
        SliceCode(if c < SLICE_CHOICES {
            c
        } else {
            SLICE_CHOICES - 1
        })
    }

    pub const fn code(self) -> u8 {
        self.0
    }

    pub const fn slice(self) -> Slice {
        match self.0 {
            0 => Slice::Off,
            1 => Slice::Note,
            k => Slice::At(k - 2),
        }
    }
}

/// Which of the window a ONESHOT note plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slice {
    /// The whole window.
    Off,
    /// The key picks the slice, and doesn't transpose.
    Note,
    /// Slice `k` (0–15) of 16, counted from START; the key transposes.
    At(u8),
}

impl Slice {
    /// SLICE's value (its code): 0 OFF, 1 NOTE, 2–17 slices 1–16. Past
    /// the last, the last.
    pub fn of(v: f32) -> Slice {
        SliceCode::new(v as u8).slice()
    }

    /// The key picks it, and plays at ROOT.
    pub fn by_note(self) -> bool {
        self == Slice::Note
    }
}

/// The slice `s` of `w` that `note` plays: slice `k` is
/// `[lo + ⌊k·W/16⌋, lo + ⌊(k + 1)·W/16⌋)`, numbered from START, so a
/// reversed window's slice 1 is its top. The 16 tile the window exactly.
pub fn slice_bounds(w: Window, s: Slice, note: u8) -> Window {
    let k = match s {
        Slice::Off => return w,
        Slice::Note => (i32::from(note) - i32::from(SLICE_NOTE_BASE)).rem_euclid(16) as u64,
        Slice::At(k) => u64::from(k.min(15)),
    };
    let k = if w.dir == Dir::Rev { 15 - k } else { k };
    let n = u64::from(w.frames());
    let edge = |k: u64| w.lo + (k * n / 16) as u32;
    // Under 16 frames (a window at the source's edge) a slice is at least
    // one frame: `edge(k) < w.hi` for every `k < 16`.
    Window {
        lo: edge(k),
        hi: edge(k + 1).max(edge(k) + 1),
        dir: w.dir,
    }
}
