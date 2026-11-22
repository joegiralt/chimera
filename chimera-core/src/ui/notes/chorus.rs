//! The chorus's FLANGER and PHASER glyphs (ADR 0317): what each does to
//! the spectrum, in the braid's box. FLANGER a comb, many narrow notches
//! evenly spaced that slide together; PHASER a few wide dips that glide.
//! RATE is how fast they travel, DEPTH how far, MIX how deep they cut (0
//! flat). Bank 2's, through `UiDoor`'s `notes` (`Ask::Chorus`); closed,
//! bank 1 draws the braid. Chorus I, II and I+II keep the braid.

use crate::dsp::chorus::ChorusMode;
use crate::dsp::fast_sin;
use crate::ui::glyph::{Braid, BraidPart, wrap};
use crate::ui::tape_strip::Paint;
use crate::ui::theme;
use core::f32::consts::PI;
use embedded_graphics::pixelcolor::Rgb565;

/// What a chorus MODE draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picture {
    /// The voices twisting round the dry line (bank 1's).
    Braid,
    /// FLANGER's comb.
    Comb,
    /// PHASER's dips.
    Dips,
}

/// `m`'s picture.
pub const fn picture(m: ChorusMode) -> Picture {
    match m {
        ChorusMode::Off | ChorusMode::JunoI | ChorusMode::JunoII | ChorusMode::JunoBoth => {
            Picture::Braid
        }
        ChorusMode::Flanger => Picture::Comb,
        ChorusMode::Phaser => Picture::Dips,
    }
}

/// The box's columns, 2 px inside it as the braid's strands.
const X0: i32 = theme::BRAID_X + 2;
const W: i32 = theme::BRAID_W - 4;
/// The flat response's row and the floor a full cut reaches.
const TOP: i32 = theme::BRAID_Y + 8;
const FLOOR: i32 = theme::BRAID_Y + theme::BRAID_H - 6;

/// The comb's notches across the box, and how many spacings the full
/// DEPTH slides them each way.
const NOTCHES: f32 = 6.0;
const COMB_SWEEP: f32 = 1.5;
/// The comb's spacing, px.
const SP: f32 = W as f32 / NOTCHES;
/// The first dip's place at rest (of the width), how far the full DEPTH
/// moves it (of that), the dips' spacing and width (of that place).
const DIP_AT: f32 = 0.22;
const DIP_SWEEP: f32 = 0.6;
const DIP_GAP: f32 = 1.6;
const DIP_W: f32 = 0.6;
const DIPS: usize = 3;

/// The sweep's phase at the braid's `frame`, radians in 0..τ: RATE sets
/// its speed as it does the braid's twist; the comb travels at half of
/// it (its crest never faster than the braid's), the dips glide at 0.35. In f64, so it stays smooth however long the
/// clock has run.
pub fn sweep(b: &Braid) -> f32 {
    let per_frame = 0.05 + 0.45 * b.rate as f64;
    let speed = match picture(b.mode) {
        Picture::Braid | Picture::Comb => 0.5,
        Picture::Dips => 0.35,
    };
    wrap(b.frame as f64 * per_frame * speed, core::f64::consts::TAU)
}

/// Where the sweep has them, -1..=1 of the full DEPTH's travel.
fn shift(b: &Braid) -> f32 {
    b.depth * fast_sin(sweep(b))
}

/// The leading notch's (the comb's third, the first dip) column at sweep
/// `s`, px from the box's left.
fn lead_at(p: Picture, s: f32) -> f32 {
    match p {
        Picture::Comb => (2.5 + COMB_SWEEP * s) * SP,
        Picture::Braid | Picture::Dips => DIP_AT * (1.0 + DIP_SWEEP * s) * W as f32,
    }
}

/// The leading notch's column now, px from the box's left: the RATE bead
/// rides it.
pub fn lead(b: &Braid) -> f32 {
    lead_at(picture(b.mode), shift(b))
}

/// How deep column `x` (from the box's left) is cut, 0..=1, before MIX.
/// The comb's notch is `(1 − sin πd)²` at `d` spacings off its centre,
/// measured from the column's nearest edge, so a notch between columns
/// still reaches the floor as it slides. A dip is `1/(1 + q²)²`, `q` its
/// widths off its centre; they combine as filters do, `1 − Π(1 − cut)`.
fn cut(p: Picture, s: f32, x: i32) -> f32 {
    match p {
        Picture::Braid => 0.0,
        Picture::Comb => {
            // Notch centres sit at half spacings, slid by the sweep.
            let u = (x as f32 + 0.5) / SP - COMB_SWEEP * s;
            let f = u - libm::floorf(u);
            let px = ((f - 0.5).abs() * SP - 0.5).max(0.0);
            let k = 1.0 - fast_sin(PI * px / SP);
            k * k
        }
        Picture::Dips => {
            let a = DIP_AT * (1.0 + DIP_SWEEP * s);
            let u = (x as f32 + 0.5) / W as f32;
            let mut pass = 1.0;
            for i in 0..DIPS {
                let q = (u - a * (1.0 + DIP_GAP * i as f32)) / (DIP_W * a);
                let r = 1.0 / (1.0 + q * q);
                pass *= 1.0 - r * r;
            }
            1.0 - pass
        }
    }
}

/// How far below the flat line column `x` (from the box's left) sits, px:
/// MIX times the cut, the full cut reaching the floor.
pub fn drop(b: &Braid, x: i32) -> i32 {
    let depth = (FLOOR - TOP) as f32;
    libm::roundf(b.mix * cut(picture(b.mode), shift(b), x) * depth) as i32
}

/// A radius-2 dot, as `draw::dot`'s.
fn dot(on: &mut dyn Paint, x: i32, y: i32, c: Rgb565) {
    on.rect(x - 1, y - 2, 3, 1, c);
    on.rect(x - 2, y - 1, 5, 3, c);
    on.rect(x - 1, y + 2, 3, 1, c);
}

/// `b`'s picture when it isn't the braid; false for the braid's modes.
/// As the braid: the floor line the dry line's colour by MIX (INK2, MID
/// or FAINT; INK with MIX in focus), the response in ACCENT 1 to 3 px by
/// MIX. The focused param is marked as the braid marks it: MODE a dot at
/// the left, RATE a bead riding the leading notch, DEPTH its sweep's
/// bounds dotted in MID, MIX the floor in INK.
pub fn draw(on: &mut dyn Paint, b: &Braid) -> bool {
    let p = picture(b.mode);
    if p == Picture::Braid {
        return false;
    }
    let level = ((b.mix * 3.0) as usize).min(2);
    let floor = match b.focus {
        Some(BraidPart::Mix) => theme::INK,
        _ => [theme::INK2, theme::MID, theme::FAINT][level],
    };
    on.rect(X0, FLOOR, W, 1, floor);
    if b.focus == Some(BraidPart::Depth) {
        for s in [-b.depth, b.depth] {
            let x = X0 + libm::roundf(lead_at(p, s)) as i32;
            for y in (TOP..FLOOR).step_by(4) {
                on.rect(x, y, 1, 1, theme::MID);
            }
        }
    }
    let s = shift(b);
    let y = |x: i32| TOP + libm::roundf(b.mix * cut(p, s, x) * (FLOOR - TOP) as f32) as i32;
    let width = level as i32 + 1;
    let mut prev = y(0);
    for x in 1..W {
        let next = y(x);
        // Each stroke a row below the last, as `draw::thick_line`: the
        // notches' walls stay one column, so they read sharp.
        for k in 0..width {
            on.line(X0 + x - 1, prev + k, X0 + x, next + k, theme::ACCENT);
        }
        prev = next;
    }
    match b.focus {
        Some(BraidPart::Mode) => dot(on, X0 + 2, y(2), theme::INK),
        Some(BraidPart::Rate) => {
            let x = (libm::roundf(lead(b)) as i32).clamp(2, W - 3);
            dot(on, X0 + x, y(x), theme::INK);
        }
        Some(BraidPart::Depth) | Some(BraidPart::Mix) | None => {}
    }
    true
}
