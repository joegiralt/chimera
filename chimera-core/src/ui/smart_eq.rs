//! What SMART EQ is doing (ADR 0296), planned for a page's viz band: on
//! MIXER › PRT › EQ the Part's four cut bands, LOW to HIGH, each hanging
//! down from its top by its live cut with the cut in dB under it; on
//! MIXER › MST › LVL every Part's four, small, under its number, its
//! role's letter below, the edited Part lit. Bank 2's (`bank2.x` links this
//! module with the UI door's groups); bank 1 reaches `plan_in` only
//! through `UiDoor::smart_eq` and draws the plan (`smart_view::paint`).
//! Closed, the band is blank.

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

/// `h` px of a cut's depth, by its square root: `CutDb::MAX` (27 dB,
/// AMOUNT's OVER) is the whole box, DEEP's 12 dB two thirds of it, and a
/// polite 4 dB still two fifths, so every range reads.
fn depth(c: CutDb, h: i32) -> i32 {
    (h as f32 * crate::dsp::algo::math::sqrt(c.get() / CutDb::MAX.get()) + 0.5) as i32
}

/// A plan being written: past its room, nothing more is added.
struct Pen<'a>(&'a mut Plan);

/// `Text`'s bytes as they are pushed: no `core::fmt` (its `Write` vtable
/// is bank 1's rodata pointing into bank 2, which the split refuses). Past
/// `Text::CAP`, nothing more.
struct Bytes<'a>(&'a mut Text);

impl Bytes<'_> {
    fn str(&mut self, s: &str) -> &mut Self {
        for &b in s.as_bytes() {
            self.byte(b);
        }
        self
    }

    fn byte(&mut self, b: u8) -> &mut Self {
        let at = usize::from(self.0.len);
        if let Some(slot) = self.0.bytes.get_mut(at) {
            *slot = b;
            self.0.len += 1;
        }
        self
    }

    /// `n`, 0..=99, in decimal.
    fn digits(&mut self, n: u32) -> &mut Self {
        if n >= 10 {
            self.byte(b'0' + (n / 10 % 10) as u8);
        }
        self.byte(b'0' + (n % 10) as u8)
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
    fn text(&mut self, x: i32, y: i32, font: Font, color: Rgb565, f: impl FnOnce(&mut Bytes<'_>)) {
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
            f(&mut Bytes(t));
            self.0.n_texts += 1;
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
            w.str(name);
        });
        let cutting = c.get() >= 0.05;
        let ink = if cutting { theme::INK } else { theme::MID };
        pen.text(cx, PART_DB_Y, Font::Label, ink, |w| {
            // Tenths, as the TILT cell reads: `-2.1 dB`, `0.0 dB`.
            let tenths = (c.get() * 10.0 + 0.5) as u32;
            let sign = if cutting { "-" } else { "" };
            w.str(sign).digits(tenths / 10).byte(b'.');
            w.digits(tenths % 10).str(" dB");
        });
    }
}

/// MST › LVL's columns: the six Parts, then the reverb's return.
const ALL_X0: i32 = 20;
const ALL_DX: i32 = 32;

fn all(pen: &mut Pen<'_>, s: &Shown, sel: PartId) {
    let column = |pen: &mut Pen<'_>, i: usize, lit: bool, cuts: CutView, role: &'static str| {
        let cx = ALL_X0 + ALL_DX * i as i32;
        let fill = if lit { theme::ACCENT } else { theme::BAR_REST };
        let x0 = cx - BANDS_W / 2;
        for b in Band::ALL {
            let bx = x0 + (ALL_BAR_W + ALL_BAR_GAP) * b.index() as i32;
            let c = cuts.cut(b);
            pen.rect(bx, viz::STRIP_TOP, ALL_BAR_W, viz::STRIP_H, theme::FAINT);
            pen.rect(bx, viz::STRIP_TOP, ALL_BAR_W, depth(c, viz::STRIP_H), fill);
        }
        let ink = if lit { theme::INK } else { theme::MID };
        pen.text(cx, viz::STRIP_PAN_Y - 3, Font::Label, ink, |w| {
            w.str(role);
        });
        cx
    };
    for p in PartId::ALL {
        let lit = p == sel;
        let role = s.roles[p.index()].letter();
        let cx = column(pen, p.index(), lit, s.cuts[p.index()], role);
        let (font, ink) = if lit {
            (Font::LabelBold, theme::INK)
        } else {
            (Font::Label, theme::MID)
        };
        let n = p.index() as u32 + 1;
        pen.text(cx, viz::STRIP_NUM_Y, font, ink, |w| {
            w.digits(n);
        });
    }
    // The reverb's return, a member with a role of its own.
    let cx = column(pen, PartId::ALL.len(), false, s.wash.0, s.wash.1.letter());
    pen.text(cx, viz::STRIP_NUM_Y, Font::Label, theme::MID, |w| {
        w.str("REV");
    });
}
