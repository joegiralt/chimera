//! SMART EQ's viz band, bank 1's side (ADR 0296): what the band shows,
//! read from the audio's live cuts (`SMART_VIEW`) and the Parts' roles,
//! its dirty key, and the one call through the UI door that paints it
//! (`smart_eq::plan_in`, bank 2's): bank 1 draws its plan with the
//! display's own helpers. Closed, nothing is drawn.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::hw::MAX_PARTS;
use crate::part_eq::smart::{CutView, Role, SMART_VIEW};
use crate::project::PartId;

/// Which of the two views.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shows {
    /// PRT › EQ: this Part's four bands.
    Part(PartId),
    /// MST › LVL: every Part, this one lit.
    All(PartId),
}

/// What the band draws: read by bank 1 from the audio's view and the
/// project's roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shown {
    pub shows: Shows,
    pub cuts: [CutView; MAX_PARTS],
    pub roles: [Role; MAX_PARTS],
    /// The reverb return's cuts and role: MST › LVL's last column.
    pub wash: (CutView, Role),
}

impl Shown {
    /// The audio's live cuts now, with the Parts' `roles` and the reverb
    /// return's.
    pub fn now(shows: Shows, roles: [Role; MAX_PARTS], wash: Role) -> Shown {
        Shown {
            shows,
            cuts: PartId::ALL.map(|p| SMART_VIEW.read(p)),
            roles,
            wash: (SMART_VIEW.read_wash(), wash),
        }
    }

    /// The band's dirty key.
    pub fn key(&self) -> u32 {
        let (tag, p) = match self.shows {
            Shows::Part(p) => (0u32, p),
            Shows::All(p) => (1, p),
        };
        let roles = self
            .roles
            .iter()
            .fold(0u32, |k, r| k << 2 | u32::from(r.code()));
        let mut k = tag << 31 ^ (p.index() as u32) << 28 ^ roles;
        for c in self.cuts {
            k = k.rotate_left(7) ^ c.word();
        }
        k.rotate_left(7) ^ self.wash.0.word() ^ u32::from(self.wash.1.code()) << 13
    }
}

/// A text's font in a `Plan`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Font {
    Label,
    LabelBold,
}

/// A short text, centred on `x`.
#[derive(Clone, Copy, Debug)]
pub struct Text {
    pub bytes: [u8; Text::CAP],
    pub len: u8,
    pub x: i16,
    pub y: i16,
    pub font: Font,
    pub color: Rgb565,
}

impl Text {
    pub const CAP: usize = 10;

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

/// A filled rectangle.
#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: i16,
    pub y: i16,
    pub w: i16,
    pub h: i16,
    pub color: Rgb565,
}

/// What bank 2 asks bank 1 to draw: rectangles, then texts. Bank 1 draws
/// them with the display's own `draw` helpers, so no helper is built a
/// second time for another display type.
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    pub rects: [Rect; Plan::RECTS],
    pub n_rects: u8,
    pub texts: [Text; Plan::TEXTS],
    pub n_texts: u8,
}

impl Plan {
    pub const RECTS: usize = 56;
    pub const TEXTS: usize = 14;

    pub const EMPTY: Plan = Plan {
        rects: [Rect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            color: Rgb565::new(0, 0, 0),
        }; Plan::RECTS],
        n_rects: 0,
        texts: [Text {
            bytes: [0; Text::CAP],
            len: 0,
            x: 0,
            y: 0,
            font: Font::Label,
            color: Rgb565::new(0, 0, 0),
        }; Plan::TEXTS],
        n_texts: 0,
    };
}

/// The band through the UI door, drawn from its plan; closed, blank.
pub fn paint<D>(d: &mut D, s: &Shown)
where
    D: DrawTarget<Color = Rgb565>,
{
    use crate::ui::{draw, theme};
    let mut plan = Plan::EMPTY;
    let ask = crate::ui::part_meter::Ask::Cuts(s, &mut plan);
    if crate::bank2::ui_door(|u| u.meters(ask)).is_none() {
        return;
    }
    for r in &plan.rects[..usize::from(plan.n_rects).min(Plan::RECTS)] {
        let (x, y, w, h) = (r.x.into(), r.y.into(), r.w.into(), r.h.into());
        draw::fill_rect(d, x, y, w, h, r.color);
    }
    for t in &plan.texts[..usize::from(plan.n_texts).min(Plan::TEXTS)] {
        let font = match t.font {
            Font::Label => &theme::FONT_LABEL,
            Font::LabelBold => &theme::FONT_LABEL_BOLD,
        };
        draw::text_center(d, font, t.as_str(), t.x.into(), t.y.into(), t.color, 0);
    }
}
