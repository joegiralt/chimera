//! The prompt (spec § Screens): a teal-outlined panel with the question,
//! the reason in the route colour, and two or three pills. A picks, SEQ
//! confirms, MENU is always CANCEL. Each prompt answers in its own enum,
//! whose `ALL` is the pills' order: a pill's place never decides its meaning.

use core::fmt::{Debug, Write};
use core::marker::PhantomData;

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

/// Two or three, in order: nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opts<T> {
    Two([T; 2]),
    Three([T; 3]),
}

impl<T: Copy> Opts<T> {
    pub fn as_slice(&self) -> &[T] {
        match self {
            Opts::Two(o) => o,
            Opts::Three(o) => o,
        }
    }

    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Opts<U> {
        match self {
            Opts::Two(o) => Opts::Two(o.map(f)),
            Opts::Three(o) => Opts::Three(o.map(f)),
        }
    }
}

/// A prompt's options, as an enum.
pub trait Answers: Copy + Eq + Debug + 'static {
    /// In the pills' order.
    const ALL: Opts<Self>;
    fn label(self) -> &'static str;
}

macro_rules! answers {
    ($(#[$m:meta])* $name:ident { $($v:ident => $l:literal),+ $(,)? } $opts:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($v),+
        }

        impl Answers for $name {
            const ALL: Opts<Self> = Opts::$opts([$($name::$v),+]);
            fn label(self) -> &'static str {
                match self {
                    $($name::$v => $l),+
                }
            }
        }
    };
}

answers!(LoadAnswer { SaveThenLoad => "SAVE THEN LOAD", LoadAnyway => "LOAD ANYWAY", Cancel => "CANCEL" } Three);
answers!(ReplaceAnswer { SavePartFirst => "SAVE PART FIRST", Replace => "REPLACE", Cancel => "CANCEL" } Three);
answers!(
    /// `Update`'s pill names the Parts: `UPDATE P4`, `UPDATE ALL`.
    AlsoUsesAnswer { Update => "UPDATE", Leave => "LEAVE" } Two
);
answers!(NameExistsAnswer { KeepBoth => "KEEP BOTH", Overwrite => "OVERWRITE THAT ONE" } Two);
answers!(DeleteAnswer { Delete => "DELETE", Cancel => "CANCEL" } Two);
answers!(ClearAnswer { Clear => "CLEAR", Cancel => "CANCEL" } Two);
answers!(SaveOverAnswer { SaveOver => "SAVE OVER", Cancel => "CANCEL" } Two);
answers!(CardChangedAnswer { SaveAs => "SAVE AS", Cancel => "CANCEL" } Two);

/// A pick, or MENU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer<A> {
    Pick(A),
    Cancel,
}

/// The highlighted option of an open prompt: always one of `A::ALL`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice<A> {
    at: u8,
    _a: PhantomData<A>,
}

impl<A: Answers> Default for Choice<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Answers> Choice<A> {
    /// On the first option.
    pub fn new() -> Self {
        Choice {
            at: 0,
            _a: PhantomData,
        }
    }

    pub fn picked(&self) -> A {
        A::ALL.as_slice()[self.at as usize]
    }

    /// MENU cancels; SEQ answers with the option shown before this frame's
    /// turn; A moves, clamped.
    pub fn input(&mut self, c: &impl Controls, p: &Presses) -> Option<Answer<A>> {
        if p.menu == Some(Press::Tap) {
            return Some(Answer::Cancel);
        }
        if p.seq == Some(Press::Tap) {
            return Some(Answer::Pick(self.picked()));
        }
        let last = A::ALL.as_slice().len() as i32 - 1;
        let to = self.at as i32 + c.encoder_delta(EncoderId::A) as i32;
        self.at = to.clamp(0, last) as u8;
        None
    }
}

/// A prompt's words (plan Pre-flight 13). Names are `Name<16>`, so
/// `every_prompt_fits` covers the longest.
pub trait Prompt {
    type Answer: Answers;
    fn words(&self, question: &mut Line, reason: &mut Line);
    fn label(&self, a: Self::Answer, out: &mut Line) {
        let _ = out.write_str(a.label());
    }
}

/// What a panel draws: only `with_view` makes one, so its pick is one of
/// its options.
#[derive(Clone, Copy, Debug)]
pub struct PromptView<'a> {
    pub question: &'a str,
    pub reason: &'a str,
    options: Opts<&'a str>,
    picked: u8,
}

impl PromptView<'_> {
    pub fn options(&self) -> &[&str] {
        self.options.as_slice()
    }

    pub fn picked(&self) -> usize {
        self.picked as usize
    }
}

/// `p` as drawn with `c`'s pick, to `f`.
pub fn with_view<P: Prompt, R>(
    p: &P,
    c: &Choice<P::Answer>,
    f: impl FnOnce(&PromptView<'_>) -> R,
) -> R {
    let (mut q, mut r) = (Line::new(""), Line::new(""));
    p.words(&mut q, &mut r);
    let labels = P::Answer::ALL.map(|a| {
        let mut l = Line::new("");
        p.label(a, &mut l);
        l
    });
    let options = match &labels {
        Opts::Two([a, b]) => Opts::Two([a.as_str(), b.as_str()]),
        Opts::Three([a, b, c]) => Opts::Three([a.as_str(), b.as_str(), c.as_str()]),
    };
    f(&PromptView {
        question: q.as_str(),
        reason: r.as_str(),
        options,
        picked: c.at,
    })
}

fn name(n: ProjectName) -> crate::ui::fmt::FmtBuf {
    upper(n.as_str())
}

fn part(p: PartId) -> usize {
    p.index() + 1
}

struct Slot(SlotId);

impl core::fmt::Display for Slot {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SLOT {:02}", self.0.index() + 1)
    }
}

/// LOAD over a Modified project: `to` a file, or NEW.
pub struct Load {
    pub to: Option<ProjectName>,
    pub current: ProjectName,
}

impl Prompt for Load {
    type Answer = LoadAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let _ = match self.to {
            Some(t) => write!(q, "LOAD {}?", name(t).as_str()),
            None => q.write_str("START A NEW PROJECT?"),
        };
        let _ = write!(r, "{} HAS UNSAVED CHANGES", name(self.current).as_str());
    }
}

/// A Part's Sound replaced: from a slot, or cleared to INIT.
pub struct Replace {
    pub part: PartId,
    pub to_init: bool,
}

impl Prompt for Replace {
    type Answer = ReplaceAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let p = part(self.part);
        let _ = if self.to_init {
            write!(q, "CLEAR P{p} TO INIT?")
        } else {
            write!(q, "REPLACE P{p} SOUND?")
        };
        let _ = write!(r, "P{p} IS EDITED");
    }
}

/// Saving over a slot `first` and any of `more` also use.
pub struct AlsoUses {
    pub first: PartId,
    pub more: PartSet,
    pub slot: SlotId,
}

impl Prompt for AlsoUses {
    type Answer = AlsoUsesAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let all = self.more.with(self.first);
        for x in all.iter() {
            let _ = write!(q, "P{} ", part(x));
        }
        let verb = if all.len() > 1 { "USE" } else { "USES" };
        let _ = write!(q, "ALSO {verb} {}", Slot(self.slot));
        let _ = r.write_str("IT KEEPS THE OLD SOUND");
    }
    fn label(&self, a: AlsoUsesAnswer, out: &mut Line) {
        let _ = match a {
            AlsoUsesAnswer::Update if self.more.with(self.first).len() > 1 => {
                out.write_str("UPDATE ALL")
            }
            AlsoUsesAnswer::Update => write!(out, "UPDATE P{}", part(self.first)),
            AlsoUsesAnswer::Leave => out.write_str(a.label()),
        };
    }
}

pub struct NameExists {
    pub slot: SlotId,
    pub name: ProjectName,
}

impl Prompt for NameExists {
    type Answer = NameExistsAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let _ = q.write_str("NAME EXISTS");
        let _ = write!(
            r,
            "{} IS NAMED {}",
            Slot(self.slot),
            name(self.name).as_str()
        );
    }
}

pub struct Delete {
    pub name: ProjectName,
}

impl Prompt for Delete {
    type Answer = DeleteAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let _ = write!(q, "DELETE {}?", name(self.name).as_str());
        let _ = r.write_str("THIS CANNOT BE UNDONE");
    }
}

pub struct Clear {
    pub name: ProjectName,
}

impl Prompt for Clear {
    type Answer = ClearAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let _ = write!(q, "CLEAR {}?", name(self.name).as_str());
        let _ = r.write_str("IT BECOMES A NEW PROJECT");
    }
}

pub struct SaveOver {
    pub name: ProjectName,
}

impl Prompt for SaveOver {
    type Answer = SaveOverAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let _ = write!(q, "SAVE OVER {}?", name(self.name).as_str());
        let _ = r.write_str("ITS CONTENTS ARE REPLACED");
    }
}

pub struct CardChanged;

impl Prompt for CardChanged {
    type Answer = CardChangedAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let _ = q.write_str("CARD CHANGED");
        let _ = r.write_str("SAVE AS A NEW PROJECT ON THIS CARD?");
    }
}

pub struct ClearSlot {
    pub slot: SlotId,
}

impl Prompt for ClearSlot {
    type Answer = ClearAnswer;
    fn words(&self, q: &mut Line, r: &mut Line) {
        let _ = write!(q, "CLEAR {}?", Slot(self.slot));
        let _ = r.write_str("NO PART USES IT");
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
/// The first question line's baseline, below the panel's top.
const TEXT_DY: i32 = 26;
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
/// Below a baseline, the label faces' descent.
const DESCENT: i32 = 3;
// Two question lines and two reason lines clear three pills.
const _: () = assert!(
    PROMPT.1 as i32 + TEXT_DY + 2 * Q_LINE + R_LINE + DESCENT
        <= PILLS_BOTTOM - (3 * PILL_STEP - (PILL_STEP - PILL_H))
);

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
        && v.options()
            .iter()
            .all(|o| w(&theme::FONT_LABEL_BOLD, o) <= PILL_TEXT_W)
}

/// The panel, opaque over its own last frame: it never needs a clear.
pub fn draw_prompt<D: DrawTarget<Color = Rgb565>>(d: &mut D, v: &PromptView<'_>) {
    let (top, h) = (PROMPT.1 as i32, (PROMPT.2 - PROMPT.1) as i32);
    draw::round_rect(d, PANEL_X, top, PANEL_W, h, PANEL_R, theme::PANEL);
    draw::round_outline(d, PANEL_X, top, PANEL_W, h, PANEL_R, theme::ACCENT);
    let mut y = top + TEXT_DY;
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
    let opts = v.options();
    let first = PILLS_BOTTOM - (opts.len() as i32 * PILL_STEP - (PILL_STEP - PILL_H));
    for (i, o) in opts.iter().enumerate() {
        let y = first + i as i32 * PILL_STEP;
        let on = i == v.picked();
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
