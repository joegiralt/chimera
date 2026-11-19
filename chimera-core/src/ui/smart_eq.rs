//! What SMART EQ is doing (ADR 0296), drawn in a page's viz band: on
//! MIXER › PRT › EQ the Part's four cut bands, LOW to HIGH, each hanging
//! down from its top by its live cut with the cut in dB under it; on
//! MIXER › MST › SMART EQ every Part's four, small, under its number, its
//! role's letter below, the edited Part lit. Bank 2's (`bank2.x` links this
//! module with the UI door's groups); bank 1 reaches `paint_in` only
//! through `UiDoor::smart_eq` (`smart_view::paint`). Closed, the band is
//! blank.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use super::orbit::door::Ink;
use super::smart_view::{Shown, Shows};
use super::{draw, theme, viz};
use crate::part_eq::smart::{Band, CutDb, CutView};
use crate::project::PartId;
use crate::ui::fmt::FmtBuf;

/// `MIXER › PRT › EQ`'s columns: centres and the bars' box.
const PART_X0: i32 = 30;
const PART_DX: i32 = 60;
const PART_LABEL_Y: i32 = 128;
const PART_BAR_Y: i32 = 136;
const PART_BAR_W: i32 = 24;
const PART_BAR_H: i32 = 34;
const PART_DB_Y: i32 = 176;
/// `MIXER › MST › SMART EQ`'s small bars in each Part's column.
const ALL_BAR_W: i32 = 4;
const ALL_BAR_GAP: i32 = 2;

/// `h` px of a cut's depth: `CutDb::MAX` the whole box.
fn depth(c: CutDb, h: i32) -> i32 {
    (h as f32 * c.get() / CutDb::MAX.get() + 0.5) as i32
}

/// The band, on bank 2's side of the door.
pub fn paint_in(d: &mut Ink<'_>, s: &Shown) {
    match s.shows {
        Shows::Part(p) => part(d, s.cuts[p.index()]),
        Shows::All(sel) => all(d, s, sel),
    }
}

fn part<D: DrawTarget<Color = Rgb565>>(d: &mut D, v: CutView) {
    let mut buf = FmtBuf::new();
    for b in Band::ALL {
        let cx = PART_X0 + PART_DX * b.index() as i32;
        let c = v.cut(b);
        draw::text_center(
            d,
            &theme::FONT_LABEL,
            b.name(),
            cx,
            PART_LABEL_Y,
            theme::MID,
            0,
        );
        let x = cx - PART_BAR_W / 2;
        draw::fill_rect(d, x, PART_BAR_Y, PART_BAR_W, PART_BAR_H, theme::FAINT);
        draw::fill_rect(
            d,
            x,
            PART_BAR_Y,
            PART_BAR_W,
            depth(c, PART_BAR_H),
            theme::ACCENT,
        );
        buf.clear();
        if c.get() >= 0.05 {
            let _ = core::fmt::Write::write_str(&mut buf, "-");
        }
        crate::ui::fmt::fixed(&mut buf, c.get(), 1, " dB");
        let ink = if c.get() >= 0.05 {
            theme::INK
        } else {
            theme::MID
        };
        draw::text_center(d, &theme::FONT_LABEL, buf.as_str(), cx, PART_DB_Y, ink, 0);
    }
}

fn all<D: DrawTarget<Color = Rgb565>>(d: &mut D, s: &Shown, sel: PartId) {
    let mut num = FmtBuf::new();
    for p in PartId::ALL {
        let (x, lit) = (viz::strip_x(p.index()), p == sel);
        num.clear();
        let _ = core::fmt::Write::write_fmt(&mut num, format_args!("{}", p.index() + 1));
        let (font, ink) = if lit {
            (&theme::FONT_LABEL_BOLD, theme::INK)
        } else {
            (&theme::FONT_LABEL, theme::MID)
        };
        draw::text_center(d, font, num.as_str(), x + 4, viz::STRIP_NUM_Y, ink, 0);
        let fill = if lit { theme::ACCENT } else { theme::BAR_REST };
        let x0 = x + 4 - (BANDS_W / 2);
        for b in Band::ALL {
            let bx = x0 + (ALL_BAR_W + ALL_BAR_GAP) * b.index() as i32;
            let c = s.cuts[p.index()].cut(b);
            draw::fill_rect(d, bx, viz::STRIP_TOP, ALL_BAR_W, viz::STRIP_H, theme::FAINT);
            draw::fill_rect(
                d,
                bx,
                viz::STRIP_TOP,
                ALL_BAR_W,
                depth(c, viz::STRIP_H),
                fill,
            );
        }
        let role = s.roles[p.index()].letter();
        draw::text_center(
            d,
            &theme::FONT_LABEL,
            role,
            x + 4,
            viz::STRIP_PAN_Y - 3,
            ink,
            0,
        );
    }
}

/// The four small bars' width.
const BANDS_W: i32 = 4 * ALL_BAR_W + 3 * ALL_BAR_GAP;
