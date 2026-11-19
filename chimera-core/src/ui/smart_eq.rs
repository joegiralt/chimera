//! What SMART EQ is doing (ADR 0296), planned for a page's viz band: on
//! MIXER › PRT › EQ the Part's four cut bands, LOW to HIGH, each hanging
//! down from its top by its live cut with the cut in dB under it; on
//! MIXER › MST › LVL every Part's four, small, under its number, its
//! role's letter below, the edited Part lit. Bank 2's (`bank2.x` links this
//! module with the UI door's groups); bank 1 reaches `plan_in` only
//! through `UiDoor::smart_eq` and draws the plan (`smart_view::paint`).
//! Closed, the band is blank.

use core::fmt::Write;

use embedded_graphics::pixelcolor::Rgb565;

use super::smart_view::{Font, Plan, Rect, Shown, Shows, Text};
use super::{theme, viz};
use crate::part_eq::smart::{Band, CutDb, CutView};
use crate::project::PartId;

/// `MIXER › PRT › EQ`'s columns: centres and the bars' box.
const PART_X0: i32 = 30;
const PART_DX: i32 = 60;
const PART_LABEL_Y: i32 = 128;
const PART_BAR_Y: i32 = 136;
const PART_BAR_W: i32 = 24;
const PART_BAR_H: i32 = 34;
const PART_DB_Y: i32 = 176;
/// `MIXER › MST › LVL`'s small bars in each Part's column.
const ALL_BAR_W: i32 = 4;
const ALL_BAR_GAP: i32 = 2;
/// The four small bars' width.
const BANDS_W: i32 = 4 * ALL_BAR_W + 3 * ALL_BAR_GAP;

/// `h` px of a cut's depth: `CutDb::MAX` the whole box.
fn depth(c: CutDb, h: i32) -> i32 {
    (h as f32 * c.get() / CutDb::MAX.get() + 0.5) as i32
}

/// A plan being written: past its room, nothing more is added.
struct Pen<'a>(&'a mut Plan);

/// `Text`'s bytes as `core::fmt::Write` fills them.
struct Bytes<'a>(&'a mut Text);

impl Write for Bytes<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let at = usize::from(self.0.len);
        let end = at + s.len();
        if end > Text::CAP {
            return Err(core::fmt::Error);
        }
        self.0.bytes[at..end].copy_from_slice(s.as_bytes());
        self.0.len = end as u8;
        Ok(())
    }
}

impl Pen<'_> {
    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb565) {
        let n = usize::from(self.0.n_rects);
        if let Some(r) = self.0.rects.get_mut(n) {
            *r = Rect {
                x: x as i16,
                y: y as i16,
                w: w as i16,
                h: h as i16,
                color,
            };
            self.0.n_rects += 1;
        }
    }

    /// A text centred on `x`, written by `f`.
    fn text(
        &mut self,
        x: i32,
        y: i32,
        font: Font,
        color: Rgb565,
        f: impl FnOnce(&mut Bytes<'_>) -> core::fmt::Result,
    ) {
        let n = usize::from(self.0.n_texts);
        if let Some(t) = self.0.texts.get_mut(n) {
            *t = Text {
                bytes: [0; Text::CAP],
                len: 0,
                x: x as i16,
                y: y as i16,
                font,
                color,
            };
            if f(&mut Bytes(t)).is_ok() {
                self.0.n_texts += 1;
            }
        }
    }
}

/// The band's plan, on bank 2's side of the door.
pub fn plan_in(s: &Shown, out: &mut Plan) {
    let mut pen = Pen(out);
    match s.shows {
        Shows::Part(p) => part(&mut pen, s.cuts[p.index()]),
        Shows::All(sel) => all(&mut pen, s, sel),
    }
}

fn part(pen: &mut Pen<'_>, v: CutView) {
    for b in Band::ALL {
        let cx = PART_X0 + PART_DX * b.index() as i32;
        let c = v.cut(b);
        let x = cx - PART_BAR_W / 2;
        pen.rect(x, PART_BAR_Y, PART_BAR_W, PART_BAR_H, theme::FAINT);
        pen.rect(
            x,
            PART_BAR_Y,
            PART_BAR_W,
            depth(c, PART_BAR_H),
            theme::ACCENT,
        );
        let name = b.name();
        pen.text(cx, PART_LABEL_Y, Font::Label, theme::MID, |w| {
            w.write_str(name)
        });
        let cutting = c.get() >= 0.05;
        let ink = if cutting { theme::INK } else { theme::MID };
        pen.text(cx, PART_DB_Y, Font::Label, ink, |w| {
            // Tenths, as the TILT cell reads: `-2.1 dB`, `0.0 dB`.
            let tenths = (c.get() * 10.0 + 0.5) as u32;
            let sign = if cutting { "-" } else { "" };
            write!(w, "{sign}{}.{} dB", tenths / 10, tenths % 10)
        });
    }
}

fn all(pen: &mut Pen<'_>, s: &Shown, sel: PartId) {
    for p in PartId::ALL {
        let (x, lit) = (viz::strip_x(p.index()), p == sel);
        let (font, ink) = if lit {
            (Font::LabelBold, theme::INK)
        } else {
            (Font::Label, theme::MID)
        };
        let n = p.index() + 1;
        pen.text(x + 4, viz::STRIP_NUM_Y, font, ink, |w| write!(w, "{n}"));
        let fill = if lit { theme::ACCENT } else { theme::BAR_REST };
        let x0 = x + 4 - BANDS_W / 2;
        for b in Band::ALL {
            let bx = x0 + (ALL_BAR_W + ALL_BAR_GAP) * b.index() as i32;
            let c = s.cuts[p.index()].cut(b);
            pen.rect(bx, viz::STRIP_TOP, ALL_BAR_W, viz::STRIP_H, theme::FAINT);
            pen.rect(bx, viz::STRIP_TOP, ALL_BAR_W, depth(c, viz::STRIP_H), fill);
        }
        let role = s.roles[p.index()].letter();
        pen.text(x + 4, viz::STRIP_PAN_Y - 3, Font::Label, ink, |w| {
            w.write_str(role)
        });
    }
}
