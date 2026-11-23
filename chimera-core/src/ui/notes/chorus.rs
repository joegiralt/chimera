//! The chorus's FLANGER and PHASER glyphs (ADR 0324), objects in the
//! braid's box: FLANGER a moiré, a fixed back comb of bars and a front
//! comb sliding and stretching over it, bright where their bars line up;
//! PHASER a blocky ray gun firing pulses along its beam. Bank 2's, through
//! `UiDoor`'s `notes` (`Ask::Chorus`); closed, bank 1 draws the braid.
//! Chorus I, II and I+II keep the braid.

use crate::dsp::chorus::ChorusMode;
use crate::dsp::fast_sin;
use crate::ui::glyph::{Braid, BraidPart, wrap};
use crate::ui::tape_strip::Paint;
use crate::ui::theme;
use core::f64::consts::TAU;
use embedded_graphics::pixelcolor::Rgb565;

/// What a chorus MODE draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picture {
    /// The voices twisting round the dry line (bank 1's).
    Braid,
    /// FLANGER's two combs.
    Moire,
    /// PHASER's ray gun.
    RayGun,
}

/// `m`'s picture.
pub const fn picture(m: ChorusMode) -> Picture {
    match m {
        ChorusMode::Off | ChorusMode::JunoI | ChorusMode::JunoII | ChorusMode::JunoBoth => {
            Picture::Braid
        }
        ChorusMode::Flanger => Picture::Moire,
        ChorusMode::Phaser => Picture::RayGun,
    }
}

/// The box's columns, 2 px inside it as the braid's strands, and its top.
const X0: i32 = theme::BRAID_X + 2;
const W: i32 = theme::BRAID_W - 4;
const Y0: i32 = theme::BRAID_Y;
/// MODE's dot, under the picture at the left, in the box.
pub const MODE_DOT: (i32, i32) = (2, 51);

/// The back comb's pitch and every bar's width, px; the bars' rows.
const BACK_PITCH: i32 = 5;
const BAR_W: i32 = 2;
const BACK_ROWS: (i32, i32) = (4, 46);
const FRONT_ROWS: (i32, i32) = (9, 41);
/// How far the full DEPTH stretches the front comb's pitch.
const STRETCH: f32 = 0.18;
/// The front comb's slide, px a radian of the clock.
const SLIDE: f64 = 3.0;

/// The gun's muzzle line and its left, px in the box.
const GUN_CY: i32 = 22;
const GUN_X: i32 = 3;
/// Pulses on the beam at once, fired twice a turn of the clock.
const PULSES: usize = 4;

/// MIX as the pictures draw it: off below 0.01, else the braid's level
/// (`mix × 3`, 0 to 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mix {
    Off,
    Level(usize),
}

impl Mix {
    fn of(mix: f32) -> Self {
        if mix < 0.01 {
            Mix::Off
        } else {
            Mix::Level(((mix * 3.0) as usize).min(2))
        }
    }
}

/// Rows a front bar or a pulse is drawn on, every `n`th: solid at MIX's
/// top level, sparser below it; a focused part at MIX 0 every fourth,
/// its faint mark.
const fn rows_every(m: Mix) -> usize {
    match m {
        Mix::Off => 4,
        Mix::Level(l) => 3 - l,
    }
}

/// The clock's phase at the braid's `frame`, radians, unwrapped: RATE
/// sets its speed as it does the braid's twist (0.05 to 0.5 a frame).
fn clock(b: &Braid) -> f64 {
    b.frame as f64 * (0.05 + 0.45 * b.rate as f64)
}

/// The front comb's pitch now, px: the back's, stretched up to ±18 % by
/// DEPTH as the clock turns at half speed. In f64 until wrapped, so it
/// stays smooth however long the clock has run.
pub fn pitch(b: &Braid) -> f32 {
    let s = fast_sin(wrap(clock(b) * 0.5, TAU));
    BACK_PITCH as f32 * (1.0 + STRETCH * b.depth * s)
}

/// The front comb's first bar, px from the box's left, 0..pitch: it
/// slides right as the clock turns, a share of its own pitch, so a
/// stretch moves it smoothly too.
fn slide(b: &Braid) -> f32 {
    let bars = wrap(clock(b) * SLIDE / BACK_PITCH as f64, 1.0).min(1.0);
    bars * pitch(b)
}

/// The gun's parts at DEPTH `k`, each `(x, y, w, h)` in the box: a skinny
/// pistol at 0, a wide boxy blaster at 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gun {
    pub body: (i32, i32, i32, i32),
    pub grip: (i32, i32, i32, i32),
    /// The trigger guard's notch under the body's front.
    pub guard: (i32, i32, i32, i32),
    pub nozzle: (i32, i32, i32, i32),
    /// The beam's band, from the nozzle to the box's right.
    pub beam: (i32, i32, i32, i32),
}

impl Gun {
    pub fn at(k: f32) -> Self {
        let r = |v: f32| libm::roundf(v) as i32;
        let (bh, bw) = (r(5.0 + 7.0 * k), r(17.0 + 5.0 * k));
        let by = GUN_CY - bh / 2;
        let noz = r(bh as f32 * (0.35 + 0.25 * k)).max(2);
        let beam = r(1.0 + 5.0 * k).max(1);
        let bx = GUN_X + bw + 4;
        Self {
            body: (GUN_X, by, bw, bh),
            grip: (GUN_X + 3, by + bh, r(5.0 + 3.0 * k), r(13.0 - 2.0 * k)),
            guard: (GUN_X + bw - 4, by + bh, 3, 3),
            nozzle: (GUN_X + bw, GUN_CY - noz / 2, 4, noz),
            beam: (bx, GUN_CY - beam / 2, W - 1 - bx, beam),
        }
    }
}

/// The beam's length at DEPTH `k`, px, unrounded: the pulses are placed
/// by it from the box's right, so they never jump as the gun's rounded
/// shape grows.
fn beam_len(k: f32) -> f32 {
    (W - 1 - GUN_X - 4) as f32 - (17.0 + 5.0 * k)
}

/// Pulse `i`'s left, px from the box's left: four evenly spaced along
/// the beam, fired twice a turn of the clock.
fn pulse(b: &Braid, i: usize) -> f32 {
    let turns = wrap(clock(b) * 0.5, TAU) / core::f32::consts::TAU;
    let u = 2.0 * turns + i as f32 / PULSES as f32;
    (W - 1) as f32 - (1.0 - (u - libm::floorf(u))) * beam_len(b.depth)
}

/// Where the motion is now and the span it repeats over, px: the front
/// comb's first bar and its pitch, or the gun's first pulse and the
/// pulses' spacing. The braid's modes: none.
pub fn travel(b: &Braid) -> (f32, f32) {
    match picture(b.mode) {
        Picture::Braid => (0.0, 1.0),
        Picture::Moire => (slide(b), pitch(b)),
        Picture::RayGun => (pulse(b, 0), beam_len(b.depth) / PULSES as f32),
    }
}

/// A radius-2 dot, as `draw::dot`'s.
fn dot(on: &mut dyn Paint, x: i32, y: i32, c: Rgb565) {
    on.rect(x - 1, y - 2, 3, 1, c);
    on.rect(x - 2, y - 1, 5, 3, c);
    on.rect(x - 1, y + 2, 3, 1, c);
}

/// A rect in the box's frame.
fn rect(on: &mut dyn Paint, (x, y, w, h): (i32, i32, i32, i32), c: Rgb565) {
    on.rect(X0 + x, Y0 + y, w, h, c);
}

/// `b`'s picture when it isn't the braid; false for the braid's modes.
/// The focused param is marked in INK, as the braid marks it: MODE a dot
/// under the left; at MIX 0 a focused part MIX turns off still shows,
/// faintly.
pub fn draw(on: &mut dyn Paint, b: &Braid) -> bool {
    let mix = Mix::of(b.mix);
    match picture(b.mode) {
        Picture::Braid => return false,
        Picture::Moire => moire(on, b, mix),
        Picture::RayGun => ray_gun(on, b, mix),
    }
    if b.focus == Some(BraidPart::Mode) {
        dot(on, X0 + MODE_DOT.0, Y0 + MODE_DOT.1, theme::INK);
    }
    true
}

/// The front comb's most bars: its least pitch across the box, and one
/// each side. A bound, so a bad pitch never runs the loop away.
const FRONT_BARS: i32 = 22;

/// The back comb in MID (INK with MIX in focus: the comb the front is
/// weighed against, as the braid's dry line); the front comb's bars in
/// ACCENT, INK2 where they cover a back bar, on `rows_every` MIX's rows,
/// none at MIX 0. RATE in focus lights the front comb, DEPTH the bands
/// where the bars line up (the rest of the front FAINT); at MIX 0 just
/// those, faintly.
fn moire(on: &mut dyn Paint, b: &Braid, mix: Mix) {
    let back = if b.focus == Some(BraidPart::Mix) {
        theme::INK
    } else {
        theme::MID
    };
    let (t, bot) = BACK_ROWS;
    for x in (0..W).step_by(BACK_PITCH as usize) {
        rect(on, (x, t, BAR_W.min(W - x), bot - t), back);
    }
    let marked = matches!(b.focus, Some(BraidPart::Rate | BraidPart::Depth));
    if mix == Mix::Off && !marked {
        return;
    }
    let every = rows_every(mix);
    let (p, first) = (pitch(b), slide(b));
    let (t, bot) = FRONT_ROWS;
    for n in -1..FRONT_BARS {
        let left = libm::roundf(first + n as f32 * p) as i32;
        if left >= W {
            break;
        }
        for cx in (left..left + BAR_W).filter(|c| (0..W).contains(c)) {
            let lined_up = cx % BACK_PITCH < BAR_W;
            let c = match (b.focus, lined_up) {
                (Some(BraidPart::Rate), _) | (Some(BraidPart::Depth), true) => theme::INK,
                (Some(BraidPart::Depth), false) if mix == Mix::Off => continue,
                (Some(BraidPart::Depth), false) => theme::FAINT,
                (_, true) => theme::INK2,
                (_, false) => theme::ACCENT,
            };
            if every == 1 {
                rect(on, (cx, t, 1, bot - t), c);
            } else {
                for y in (t..bot).step_by(every) {
                    rect(on, (cx, y, 1, 1), c);
                }
            }
        }
    }
}

/// The gun in ACCENT (its guard MID, a panel slot cut in it), at DEPTH's
/// shape; the beam's band FAINT or MID by MIX and its pulses MID, ACCENT
/// or INK2, none at MIX 0. RATE in focus lights the pulses (at MIX 0 on
/// every fourth row, faintly), DEPTH the gun's outline, MIX the beam's
/// band (drawn at MIX 0 too).
fn ray_gun(on: &mut dyn Paint, b: &Braid, mix: Mix) {
    let g = Gun::at(b.depth);
    for part in [g.body, g.grip, g.nozzle] {
        rect(on, part, theme::ACCENT);
    }
    rect(on, g.guard, theme::MID);
    let (x, y, _, _) = g.body;
    rect(on, (x + 2, y + 2, 3, 1), theme::BG);
    if b.focus == Some(BraidPart::Depth) {
        for (x, y, w, h) in [g.body, g.grip, g.nozzle] {
            for edge in [
                (x, y, w, 1),
                (x, y + h - 1, w, 1),
                (x, y, 1, h),
                (x + w - 1, y, 1, h),
            ] {
                rect(on, edge, theme::INK);
            }
        }
    }
    let band = match (b.focus, mix) {
        (Some(BraidPart::Mix), _) => Some(theme::INK),
        (_, Mix::Level(l)) => Some([theme::FAINT, theme::FAINT, theme::MID][l]),
        (_, Mix::Off) => None,
    };
    if let Some(c) = band {
        rect(on, g.beam, c);
    }
    let pc = match (b.focus, mix) {
        (Some(BraidPart::Rate), _) => theme::INK,
        (_, Mix::Level(l)) => [theme::MID, theme::ACCENT, theme::INK2][l],
        (_, Mix::Off) => return,
    };
    let (bx, by, len, bh) = g.beam;
    let pw = libm::roundf(3.0 + 4.0 * b.depth) as i32;
    for i in 0..PULSES {
        let px = (libm::roundf(pulse(b, i)) as i32).max(bx);
        let w = pw.min(bx + len - px);
        if w <= 0 {
            continue;
        }
        match mix {
            Mix::Level(_) => rect(on, (px, by - 1, w, bh + 2), pc),
            Mix::Off => {
                for y in (by - 1..by + bh + 1).step_by(rows_every(mix)) {
                    rect(on, (px, y, w, 1), pc);
                }
            }
        }
    }
}
