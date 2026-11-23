// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! PIT's tuner (ADR 0201): the corner of the scope reads the pitch the
//! Part's last note sounds at, `C4 +12ct` over `261.6 Hz`, worked out from
//! the note and the Sound (`calc`), never heard off the output. Lit while
//! the Part sounds, dim between notes (the last note under the PITCH and
//! FINE set now), `REF C4` before any. Its working and its paint are bank
//! 2's (`calc`, through `UiDoor::pit` and `pit_paint`); bank 1 keeps the
//! state and the two door calls. Closed, the corner is empty.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::FontRenderer;

use crate::hw::MAX_PARTS;
use crate::ui::fmt::FmtBuf;

pub mod calc;

/// One line of the corner, set where it is drawn.
pub struct Line {
    pub text: FmtBuf,
    pub font: &'static FontRenderer,
    pub x: i32,
    /// Baseline.
    pub y: i32,
    pub color: Rgb565,
}

/// What the corner shows: its backing box (x, y, w, h), cleared under the
/// lines so the scope never runs through them, and the lines; `key`
/// fingerprints all of it, the viz band's key folds it in.
pub struct Shown {
    pub back: (i32, i32, i32, i32),
    pub lines: [Option<Line>; 2],
    pub key: u32,
}

impl Shown {
    pub const NONE: Shown = Shown {
        back: (0, 0, 0, 0),
        lines: [None, None],
        key: 0,
    };
}

/// Each Part's last live note, as it sounds after the scale's snap, and
/// what the corner shows now.
pub struct Tuner {
    last: [Option<crate::MidiNote>; MAX_PARTS],
    /// `hear`'s: whether the edited Part sounds, and `played` unread.
    #[cfg(any(test, feature = "test-support"))]
    heard: Option<bool>,
    pub shown: Shown,
}

impl Tuner {
    pub const NEW: Tuner = Tuner {
        last: [None; MAX_PARTS],
        #[cfg(any(test, feature = "test-support"))]
        heard: None,
        shown: Shown::NONE,
    };

    /// `part`'s last note is `note` and the edited Part sounds as
    /// `sounding` says, whatever the audio publishes: for the goldens and
    /// tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn hear(
        &mut self,
        part: crate::project::PartId,
        note: Option<crate::MidiNote>,
        sounding: bool,
    ) {
        self.heard = Some(sounding);
        self.last[part.index()] = note;
    }

    /// `hear`'s word on whether the edited Part sounds; `None` reads the
    /// audio's.
    fn heard(&self) -> Option<bool> {
        #[cfg(any(test, feature = "test-support"))]
        return self.heard;
        #[cfg(not(any(test, feature = "test-support")))]
        None
    }
}

/// The corner, over the scope already drawn; closed, nothing.
pub fn paint<D>(d: &mut D, s: &Shown)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut on = crate::ui::tape_strip::On(d);
    crate::bank2::ui_door(|u| u.pit_paint(&mut on, s));
}
