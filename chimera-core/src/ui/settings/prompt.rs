//! The prompt (spec § Screens): a teal-outlined panel with the question,
//! the reason in the route colour, and two or three pills. A picks, SEQ
//! confirms, MENU is always CANCEL.

use core::fmt::Write;

use chimera_hal::{Controls, EncoderId};
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;
use u8g2_fonts::FontRenderer;

use crate::name::ProjectName;
use crate::project::{Line, PartId, PartSet, SlotId};
use crate::ui::components::upper;
use crate::ui::draw;
use crate::ui::hold::{Press, Presses};
use crate::ui::region::PROMPT;
use crate::ui::theme;

/// One of a prompt's options; `Choice` never gives one past its `Count`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pick {
    First,
    Second,
    Third,
}

impl Pick {
    const ALL: [Pick; 3] = [Pick::First, Pick::Second, Pick::Third];

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Pick(Pick),
    Cancel,
}

/// How many options a prompt has: two or three, nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Two,
    Three,
}

impl Count {
    pub fn last(self) -> Pick {
        match self {
            Count::Two => Pick::Second,
            Count::Three => Pick::Third,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Options<'a> {
    Two([&'a str; 2]),
    Three([&'a str; 3]),
}

impl<'a> Options<'a> {
    pub fn as_slice(&self) -> &[&'a str] {
        match self {
            Options::Two(o) => o,
            Options::Three(o) => o,
        }
    }

    pub fn count(&self) -> Count {
        match self {
            Options::Two(_) => Count::Two,
            Options::Three(_) => Count::Three,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PromptView<'a> {
    pub question: &'a str,
    pub reason: &'a str,
    pub options: Options<'a>,
}

/// The highlighted option of an open prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pick: Pick,
    count: Count,
}

impl Choice {
    /// On the first option.
    pub fn new(count: Count) -> Self {
        Choice {
            pick: Pick::First,
            count,
        }
    }

    pub fn pick(&self) -> Pick {
        self.pick
    }

    /// MENU cancels; SEQ answers with the option shown before this frame's
    /// turn; A moves, clamped.
    pub fn input(&mut self, c: &impl Controls, p: &Presses) -> Option<Answer> {
        if p.menu == Some(Press::Tap) {
            return Some(Answer::Cancel);
        }
        if p.seq == Some(Press::Tap) {
            return Some(Answer::Pick(self.pick));
        }
        let to = self.pick as i32 + c.encoder_delta(EncoderId::A) as i32;
        self.pick = Pick::ALL[to.clamp(0, self.count.last() as i32) as usize];
        None
    }
}

/// Every prompt's words (plan Pre-flight 13). Names are `Name<16>`, so
/// `every_prompt_fits` covers the longest.
#[derive(Clone, Copy, Debug)]
pub enum Wording {
    /// `to` a file, or NEW; `current` is the loaded project.
    Load {
        to: Option<ProjectName>,
        current: ProjectName,
    },
    /// A Part's Sound replaced: from a slot, or cleared to INIT.
    Replace {
        part: PartId,
        to_init: bool,
    },
    /// Saving over a slot `first` and any of `more` also use.
    AlsoUses {
        first: PartId,
        more: PartSet,
        slot: SlotId,
    },
    NameExists {
        slot: SlotId,
        name: ProjectName,
    },
    Delete {
        name: ProjectName,
    },
    Clear {
        name: ProjectName,
    },
    SaveOver {
        name: ProjectName,
    },
    CardChanged,
    ClearSlot {
        slot: SlotId,
    },
}

struct Slot(SlotId);

impl core::fmt::Display for Slot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SLOT {:02}", self.0.index() + 1)
    }
}

impl Wording {
    pub fn count(self) -> Count {
        match self {
            Wording::Load { .. } | Wording::Replace { .. } => Count::Three,
            _ => Count::Two,
        }
    }

    /// The prompt as drawn, to `f`.
    pub fn with_view<R>(self, f: impl FnOnce(&PromptView<'_>) -> R) -> R {
        let (mut q, mut r, mut o) = (Line::new(""), Line::new(""), Line::new(""));
        let n = |n: ProjectName| upper(n.as_str());
        let p = |p: PartId| p.index() + 1;
        let _ = match self {
            Wording::Load { to, current } => {
                let _ = match to {
                    Some(t) => write!(q, "LOAD {}?", n(t).as_str()),
                    None => q.write_str("START A NEW PROJECT?"),
                };
                write!(r, "{} HAS UNSAVED CHANGES", n(current).as_str())
            }
            Wording::Replace { part, to_init } => {
                let _ = if to_init {
                    write!(q, "CLEAR P{} TO INIT?", p(part))
                } else {
                    write!(q, "REPLACE P{} SOUND?", p(part))
                };
                write!(r, "P{} IS EDITED", p(part))
            }
            Wording::AlsoUses { first, more, slot } => {
                let all = more.with(first);
                for x in all.iter() {
                    let _ = write!(q, "P{} ", p(x));
                }
                let verb = if all.len() > 1 { "USE" } else { "USES" };
                let _ = write!(q, "ALSO {verb} {}", Slot(slot));
                let _ = if all.len() > 1 {
                    o.write_str("UPDATE ALL")
                } else {
                    write!(o, "UPDATE P{}", p(first))
                };
                r.write_str("IT KEEPS THE OLD SOUND")
            }
            Wording::NameExists { slot, name } => {
                let _ = q.write_str("NAME EXISTS");
                write!(r, "{} IS NAMED {}", Slot(slot), n(name).as_str())
            }
            Wording::Delete { name } => {
                let _ = write!(q, "DELETE {}?", n(name).as_str());
                r.write_str("THIS CANNOT BE UNDONE")
            }
            Wording::Clear { name } => {
                let _ = write!(q, "CLEAR {}?", n(name).as_str());
                r.write_str("IT BECOMES A NEW PROJECT")
            }
            Wording::SaveOver { name } => {
                let _ = write!(q, "SAVE OVER {}?", n(name).as_str());
                r.write_str("ITS CONTENTS ARE REPLACED")
            }
            Wording::CardChanged => {
                let _ = q.write_str("CARD CHANGED");
                r.write_str("SAVE AS A NEW PROJECT ON THIS CARD?")
            }
            Wording::ClearSlot { slot } => {
                let _ = write!(q, "CLEAR {}?", Slot(slot));
                r.write_str("NO PART USES IT")
            }
        };
        let options = match self {
            Wording::Load { .. } => Options::Three(["SAVE THEN LOAD", "LOAD ANYWAY", "CANCEL"]),
            Wording::Replace { .. } => Options::Three(["SAVE PART FIRST", "REPLACE", "CANCEL"]),
            Wording::AlsoUses { .. } => Options::Two([o.as_str(), "LEAVE"]),
            Wording::NameExists { .. } => Options::Two(["KEEP BOTH", "OVERWRITE THAT ONE"]),
            Wording::Delete { .. } => Options::Two(["DELETE", "CANCEL"]),
            Wording::Clear { .. } | Wording::ClearSlot { .. } => Options::Two(["CLEAR", "CANCEL"]),
            Wording::SaveOver { .. } => Options::Two(["SAVE OVER", "CANCEL"]),
            Wording::CardChanged => Options::Two(["SAVE AS", "CANCEL"]),
        };
        f(&PromptView {
            question: q.as_str(),
            reason: r.as_str(),
            options,
        })
    }
}

const PANEL_X: i32 = 14;
const PANEL_W: i32 = 212;
const PANEL_R: u32 = 10;
/// The text's width inside the panel.
pub const TEXT_W: i32 = 200;
const CX: i32 = PANEL_X + PANEL_W / 2;
const Q_FONT: &FontRenderer = &theme::FONT_LABEL_BOLD;
const R_FONT: &FontRenderer = &theme::FONT_LABEL;
const Q_LINE: i32 = 14;
const R_LINE: i32 = 12;
const PILL_X: i32 = 36;
const PILL_W: i32 = 168;
const PILL_H: i32 = 26;
const PILL_STEP: i32 = 32;
const PILL_TEXT_W: i32 = PILL_W - 2 * 8;
const PILL_BASELINE: i32 = 17;
/// The last pill's bottom, above the panel's.
const PILLS_BOTTOM: i32 = PROMPT.2 as i32 - 14;

/// `s` in one line, or two broken at the last space that fits.
fn wrap<'s>(font: &FontRenderer, s: &'s str) -> (&'s str, Option<&'s str>) {
    let fits = |t: &str| draw::text_width(font, t, theme::LABEL_TRACKING) <= TEXT_W;
    if fits(s) {
        return (s, None);
    }
    let cut = s
        .match_indices(' ')
        .map(|(i, _)| i)
        .take_while(|&i| fits(&s[..i]))
        .last();
    match cut {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    }
}

/// Every line within the panel in at most two lines each, and every option
/// within its pill.
pub fn fits(v: &PromptView<'_>) -> bool {
    let w = |f, s| draw::text_width(f, s, theme::LABEL_TRACKING);
    let lines = |f, s| {
        let (a, b) = wrap(f, s);
        w(f, a) <= TEXT_W && b.is_none_or(|b| w(f, b) <= TEXT_W)
    };
    lines(Q_FONT, v.question)
        && lines(R_FONT, v.reason)
        && v.options
            .as_slice()
            .iter()
            .all(|o| w(&theme::FONT_LABEL_BOLD, o) <= PILL_TEXT_W)
}

/// The panel, opaque over its own last frame: it never needs a clear.
pub fn draw_prompt<D: DrawTarget<Color = Rgb565>>(d: &mut D, v: &PromptView<'_>, pick: Pick) {
    let (top, h) = (PROMPT.1 as i32, (PROMPT.2 - PROMPT.1) as i32);
    draw::round_rect(d, PANEL_X, top, PANEL_W, h, PANEL_R, theme::PANEL);
    draw::round_outline(d, PANEL_X, top, PANEL_W, h, PANEL_R, theme::ACCENT);
    let mut y = top + 26;
    let mut put = |d: &mut D, font, s: Option<&str>, step, color| {
        if let Some(s) = s {
            draw::text_center(d, font, s, CX, y, color, theme::LABEL_TRACKING);
            y += step;
        }
    };
    let (q1, q2) = wrap(Q_FONT, v.question);
    put(d, Q_FONT, Some(q1), Q_LINE, theme::INK);
    put(d, Q_FONT, q2, Q_LINE, theme::INK);
    let (r1, r2) = wrap(R_FONT, v.reason);
    put(d, R_FONT, Some(r1), R_LINE, theme::WARN);
    put(d, R_FONT, r2, R_LINE, theme::WARN);
    let opts = v.options.as_slice();
    let first = PILLS_BOTTOM - (opts.len() as i32 * PILL_STEP - (PILL_STEP - PILL_H));
    for (i, o) in opts.iter().enumerate() {
        let y = first + i as i32 * PILL_STEP;
        let on = i == pick.index();
        if on {
            draw::pill(d, PILL_X, y, PILL_W, PILL_H, theme::ACCENT);
        } else {
            draw::pill(d, PILL_X, y, PILL_W, PILL_H, theme::BG);
            draw::round_outline(
                d,
                PILL_X,
                y,
                PILL_W,
                PILL_H,
                PILL_H as u32 / 2,
                theme::PILL_EDGE,
            );
        }
        let ink = if on { theme::BG } else { theme::INK2 };
        draw::text_center(
            d,
            &theme::FONT_LABEL_BOLD,
            o,
            CX,
            y + PILL_BASELINE,
            ink,
            theme::LABEL_TRACKING,
        );
    }
}
