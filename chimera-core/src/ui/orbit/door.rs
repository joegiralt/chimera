// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! ORBIT's pages behind bank 2's door (ADR 0208): the viz bands,
//! ORBIT's own cells' text, its focus band and the keys that redraw them.
//! `draw` holds them, drawn on `raster`'s `Ink`: one display type, so each
//! is one function the door can name, and with `bank2` and `orbit` `draw`
//! and `raster` link into `.bank2` (`bank2.x`'s ORBIT globs); this module,
//! `Ink` and the helpers built for it are bank 1's. The table sits
//! at its own slot after the door (`ORBIT_UI_DOOR_ADDR`), beside TAPE's
//! `UiDoor`. Elsewhere bank 1
//! calls them directly.
//!
//! Closed, the bands are blank, a cell reads `UPDATE` and the focus band
//! `NEEDS UPDATE` (`closed_focus`, `bank2::closed_text`), and the header
//! warns `UPDATE` (`bank2::closed_orbit`).

use core::convert::Infallible;

use embedded_graphics::Pixel;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Dimensions;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::primitives::Rectangle;

use super::OrbitCell;
use super::draw::{self, Focus, OrbitFrame};
use super::raster;
use crate::orbit::RingIdx;
use crate::ui::block_def::BlockDef;
use crate::ui::fmt::FmtBuf;
use crate::ui::view::SlotCtx;

/// A display behind a `dyn`.
pub trait Paint {
    fn bounds(&self) -> Rectangle;
    fn fill(&mut self, area: &Rectangle, color: Rgb565);
    fn pixels(&mut self, px: &mut dyn Iterator<Item = Pixel<Rgb565>>);
}

impl<D: DrawTarget<Color = Rgb565>> Paint for D {
    fn bounds(&self) -> Rectangle {
        self.bounding_box()
    }

    fn fill(&mut self, area: &Rectangle, color: Rgb565) {
        let _ = self.fill_solid(area, color);
    }

    fn pixels(&mut self, px: &mut dyn Iterator<Item = Pixel<Rgb565>>) {
        let _ = self.draw_iter(px);
    }
}

/// What ORBIT draws on: any display, as `Paint`. Bank 1's, with every
/// shape, font and `ui::draw` helper built for it: only `draw`'s own
/// functions and `raster`'s are bank 2's.
pub struct Ink<'a>(&'a mut dyn Paint);

impl Dimensions for Ink<'_> {
    fn bounding_box(&self) -> Rectangle {
        self.0.bounds()
    }
}

impl DrawTarget for Ink<'_> {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Infallible>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        self.0.pixels(&mut pixels.into_iter());
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Infallible> {
        self.0.fill(area, color);
        Ok(())
    }
}

/// A band of a ring (RHYTHM, VOICE, NOTES, ORB).
type RingBand = fn(&mut Ink<'_>, &OrbitFrame, RingIdx);

/// The entries, private: called only through `through`, so no pointer
/// into bank 2 leaves it.
#[repr(C)]
pub struct OrbitUiDoor {
    magic: Magic,
    rhythm: RingBand,
    voice: RingBand,
    notes: RingBand,
    orb: RingBand,
    circle: fn(&mut Ink<'_>, &OrbitFrame, f32),
    chords: fn(&mut Ink<'_>, &OrbitFrame),
    scenes: fn(&mut Ink<'_>, &OrbitFrame),
    cell_text: fn(OrbitCell, &OrbitFrame, RingIdx) -> FmtBuf,
    #[allow(clippy::type_complexity)]
    focus:
        fn(&BlockDef, usize, &SlotCtx, &OrbitFrame, bool, &dyn Fn(usize) -> f32) -> Option<Focus>,
    viz_key: fn(&BlockDef, &OrbitFrame, RingIdx, f32) -> u32,
}

/// `OrbitUiDoor`'s mark: a table at the slot that isn't this one fails
/// the boot's door check (`door_ok`), and bank 2 reads corrupt.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Magic(u32);

const MAGIC: Magic = Magic(0xB2C0_0B17);

impl OrbitUiDoor {
    pub const OWN: OrbitUiDoor = OrbitUiDoor {
        magic: MAGIC,
        rhythm: raster::rhythm,
        voice: raster::voice,
        notes: raster::notes,
        orb: raster::orb,
        circle: raster::circle,
        chords: raster::chords,
        scenes: raster::scenes,
        cell_text: draw::cell_text,
        focus: raster::focus,
        viz_key: draw::viz_key,
    };
}

#[cfg(all(feature = "bank2", feature = "orbit"))]
// SAFETY: `bank2.x` links `ORBIT_UI_DOOR`, an `OrbitUiDoor`, at
// `ORBIT_UI_DOOR_ADDR` in every firmware build with `orbit` (its assert,
// `__bank2_orbit_ui_door`).
const SLOT: chimera_engine::bank2::DoorSlot<OrbitUiDoor> =
    unsafe { chimera_engine::bank2::DoorSlot::new(chimera_engine::bank2::ORBIT_UI_DOOR_ADDR) };

#[cfg(all(feature = "bank2", feature = "orbit"))]
#[unsafe(link_section = ".bank2.door.orbit_ui")]
#[used]
static ORBIT_UI_DOOR: OrbitUiDoor = OrbitUiDoor::OWN;

/// ORBIT's pages' table in judged door `j` answers (the boot's door
/// check, run before the door opens); without `orbit` there is none.
#[cfg(feature = "bank2")]
pub fn door_ok(j: &chimera_engine::bank2::Judged) -> bool {
    #[cfg(feature = "orbit")]
    {
        j.with_slot(SLOT, |d| d.magic == MAGIC)
    }
    #[cfg(not(feature = "orbit"))]
    {
        let _ = j;
        true
    }
}

/// `f` on the table: bank 2's through the door on the unit, `None`
/// closed or without `orbit`; this build's own on a host. A host test under `bank2`
/// answers as the unit does: only while the door cell is open.
#[inline(always)]
fn through<R>(f: impl FnOnce(&OrbitUiDoor) -> R) -> Option<R> {
    #[cfg(all(feature = "bank2", feature = "orbit", target_os = "none"))]
    {
        crate::bank2::DOOR_CELL.with_slot(SLOT, f)
    }
    // A firmware build without ORBIT never shows its pages: none links.
    #[cfg(all(not(feature = "orbit"), target_os = "none"))]
    {
        let _ = f;
        None
    }
    #[cfg(not(target_os = "none"))]
    {
        #[cfg(feature = "bank2")]
        if !crate::bank2::DOOR_CELL.is_open() {
            return None;
        }
        Some(f(&OrbitUiDoor::OWN))
    }
}

/// A band drawn through the door on `d`.
fn band<D: DrawTarget<Color = Rgb565>>(d: &mut D, f: impl FnOnce(&OrbitUiDoor, &mut Ink<'_>)) {
    let mut ink = Ink(d);
    through(|u| f(u, &mut ink));
}

pub fn rhythm<D: DrawTarget<Color = Rgb565>>(d: &mut D, f: &OrbitFrame, r: RingIdx) {
    band(d, |u, ink| (u.rhythm)(ink, f, r));
}

pub fn voice<D: DrawTarget<Color = Rgb565>>(d: &mut D, f: &OrbitFrame, r: RingIdx) {
    band(d, |u, ink| (u.voice)(ink, f, r));
}

pub fn notes<D: DrawTarget<Color = Rgb565>>(d: &mut D, f: &OrbitFrame, r: RingIdx) {
    band(d, |u, ink| (u.notes)(ink, f, r));
}

pub fn orb<D: DrawTarget<Color = Rgb565>>(d: &mut D, f: &OrbitFrame, sel: RingIdx) {
    band(d, |u, ink| (u.orb)(ink, f, sel));
}

pub fn circle<D: DrawTarget<Color = Rgb565>>(d: &mut D, f: &OrbitFrame, cursor: f32) {
    band(d, |u, ink| (u.circle)(ink, f, cursor));
}

pub fn chords<D: DrawTarget<Color = Rgb565>>(d: &mut D, f: &OrbitFrame) {
    band(d, |u, ink| (u.chords)(ink, f));
}

pub fn scenes<D: DrawTarget<Color = Rgb565>>(d: &mut D, f: &OrbitFrame) {
    band(d, |u, ink| (u.scenes)(ink, f));
}

/// ORBIT's own cell's text; closed, `UPDATE`.
pub fn cell_text(cell: OrbitCell, f: &OrbitFrame, ring: RingIdx) -> FmtBuf {
    through(|u| (u.cell_text)(cell, f, ring)).unwrap_or_else(|| {
        let mut b = FmtBuf::new();
        let _ = core::fmt::Write::write_str(&mut b, crate::bank2::NEEDS_UPDATE_CELL);
        b
    })
}

/// ORBIT's reading of `slot` (`draw::focus`); closed, its own cells'
/// `closed_focus`.
pub fn focus(
    def: &BlockDef,
    slot: usize,
    ctx: &SlotCtx,
    f: &OrbitFrame,
    hints: bool,
    values: impl Fn(usize) -> f32,
) -> Option<Focus> {
    through(|u| (u.focus)(def, slot, ctx, f, hints, &values))
        .unwrap_or_else(|| super::closed_focus(def, slot, ctx))
}

/// What the viz draws, for its region's key; closed, 0.
pub fn viz_key(def: &BlockDef, f: &OrbitFrame, r: RingIdx, cursor: f32) -> u32 {
    through(|u| (u.viz_key)(def, f, r, cursor)).unwrap_or(0)
}
