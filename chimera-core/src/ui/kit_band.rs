// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! KIT's band (ADR 0265): the twelve pads, each its machine's icon (an
//! instrument mark and its family's letter, `docs/mockups/drum-glyphs.html`)
//! and its choke group; the selected pad framed, a sounding one lit.

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use crate::dsp::drum::types::{ChokeGroup, Kit, Machine, PadId, PadSet};
use crate::ui::{draw, region, theme};

/// What a machine's icon shows of it: 22 marks cover the 28 machines;
/// slice 1's twelve so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Instrument {
    Kick,
    Snare,
    Tom,
    Clap,
    Cowbell,
    Cymbal,
    Hat,
    Jingle,
    Shaker,
    Sine,
    Noise,
    Impulse,
}

/// A machine's family: the badge's letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Tone,
    Metal,
    Body,
    Plain,
}

/// `m`'s instrument mark.
pub const fn instrument(m: Machine) -> Instrument {
    match m {
        Machine::ToneKick => Instrument::Kick,
        Machine::ToneSnare => Instrument::Snare,
        Machine::ToneTom => Instrument::Tom,
        Machine::ToneClap => Instrument::Clap,
        Machine::ToneCowbell => Instrument::Cowbell,
        Machine::ToneCymbal => Instrument::Cymbal,
        Machine::MetalHat => Instrument::Hat,
        Machine::MetalJingle => Instrument::Jingle,
        Machine::BodyShaker => Instrument::Shaker,
        Machine::PlainSine => Instrument::Sine,
        Machine::PlainNoise => Instrument::Noise,
        Machine::PlainImpulse => Instrument::Impulse,
    }
}

/// `m`'s family.
pub const fn family(m: Machine) -> Family {
    match m {
        Machine::ToneKick
        | Machine::ToneSnare
        | Machine::ToneTom
        | Machine::ToneClap
        | Machine::ToneCowbell
        | Machine::ToneCymbal => Family::Tone,
        Machine::MetalHat | Machine::MetalJingle => Family::Metal,
        Machine::BodyShaker => Family::Body,
        Machine::PlainSine | Machine::PlainNoise | Machine::PlainImpulse => Family::Plain,
    }
}

/// `m`'s name's last word, its instrument (`SNARE`, `COWBELL`): where
/// the whole name doesn't fit, the icon's badge says the family.
pub fn word(m: Machine) -> &'static str {
    let name = crate::dsp::drum::laws::spec(m).name;
    name.rsplit(' ').next().unwrap_or(name)
}

/// A pixel of a mark: none, the full colour, or its dim second tone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    None,
    Full,
    Dim,
}

/// An instrument mark's width and height.
pub const MARK_W: i32 = 14;
pub const MARK_H: i32 = 12;

/// A 14 x 12 mark, two bits a pixel (`Tone`), pixel `x` at bits `2x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark([u32; MARK_H as usize]);

impl Mark {
    /// From the mockup's rows: `#` full, `+` dim, `.` none. A row of
    /// another width or character fails the build.
    const fn of(rows: [&str; MARK_H as usize]) -> Mark {
        let mut m = [0; MARK_H as usize];
        let mut y = 0;
        while y < rows.len() {
            let b = rows[y].as_bytes();
            assert!(b.len() == MARK_W as usize, "a mark row is 14 px");
            let mut x = 0;
            while x < b.len() {
                m[y] |= match b[x] {
                    b'.' => 0,
                    b'#' => 1,
                    b'+' => 2,
                    _ => panic!("a mark pixel is . # or +"),
                } << (2 * x);
                x += 1;
            }
            y += 1;
        }
        Mark(m)
    }

    pub fn at(&self, x: i32, y: i32) -> Tone {
        match self.0[y as usize] >> (2 * x) & 3 {
            1 => Tone::Full,
            2 => Tone::Dim,
            _ => Tone::None,
        }
    }
}

// The instruments' marks: the mockup's pixels.
static KICK: Mark = Mark::of([
    "....######....",
    "..##++++++##..",
    ".#++++++++++#.",
    "#++++++++++++#",
    "#++++++++++++#",
    "#++++++++++++#",
    "#+++++##+++++#",
    "#++++#..#++++#",
    ".#++++##++++#.",
    "..##++++++##..",
    "#...######...#",
    "#............#",
]);
static SNARE: Mark = Mark::of([
    "##..........##",
    "..##......##..",
    "....##..##....",
    "......##......",
    "....##..##....",
    "##############",
    "#++++++++++++#",
    "#+#++#++#++#+#",
    "#++++++++++++#",
    "##############",
    "#.#.#.#.#.#.#.",
    ".#.#.#.#.#.#.#",
]);
static TOM: Mark = Mark::of([
    ".############.",
    ".#++++++++++#.",
    ".#+#++++++#+#.",
    ".#++++++++++#.",
    ".#++++++++++#.",
    ".#++++++++++#.",
    ".#+#++++++#+#.",
    ".#++++++++++#.",
    ".############.",
    ".#..........#.",
    "#............#",
    "#............#",
]);
static CLAP: Mark = Mark::of([
    ".#..........#.",
    "..#..#..#..#..",
    "....##..##....",
    "...###..###...",
    "...###..###...",
    "..####..####..",
    "..####..####..",
    ".#####..#####.",
    "..####..####..",
    "...###..###...",
    "...##....##...",
    "..............",
]);
static COWBELL: Mark = Mark::of([
    "......##......",
    ".....#..#.....",
    "....######....",
    "....#....#....",
    "...#......#...",
    "...#......#...",
    "..#........#..",
    "..#........#..",
    ".#..........#.",
    ".############.",
    "......##......",
    "..............",
]);
static CYMBAL: Mark = Mark::of([
    "......##......",
    "....######....",
    ".##++++++++##.",
    "##############",
    "......##......",
    "......##......",
    "......##......",
    "......##......",
    "......##......",
    ".....####.....",
    "...##....##...",
    ".##........##.",
]);
static HAT: Mark = Mark::of([
    "......##......",
    "..##########..",
    "##..........##",
    "..##########..",
    "..##########..",
    "##..........##",
    "..##########..",
    "......##......",
    "......##......",
    "....######....",
    "...#......#...",
    "..............",
]);
static JINGLE: Mark = Mark::of([
    "...########...",
    "..#........#..",
    ".#.##....##.#.",
    "#..##....##..#",
    "#............#",
    "#.##......##.#",
    "#.##......##.#",
    "#............#",
    ".#..##..##..#.",
    "..#.##..##.#..",
    "...########...",
    "..............",
]);
static SHAKER: Mark = Mark::of([
    "...#####......",
    "..#+++++#..#..",
    ".#++.+.++#....",
    ".#+.+.+.+#..#.",
    ".#++.+.++#....",
    "..#+++++#..#..",
    "...#####......",
    "......##......",
    ".......##.....",
    "........##....",
    ".........##...",
    "..........##..",
]);
static SINE: Mark = Mark::of([
    "..............",
    "..###.........",
    ".#...#........",
    "#.....#.......",
    "#.....#.......",
    ".......#.....#",
    ".......#.....#",
    "........#...#.",
    ".........###..",
    "..............",
    "..............",
    "..............",
]);
static NOISE: Mark = Mark::of([
    "..............",
    "...#......#...",
    "...#.#....#.#.",
    ".#.#.#..#.#.#.",
    ".#####.###.###",
    "##############",
    ".#.###.#.###.#",
    "...#.#.#...#..",
    "...#...#......",
    "..............",
    "..............",
    "..............",
]);
static IMPULSE: Mark = Mark::of([
    "#.............",
    "#....#........",
    "#....#........",
    "#....#...#....",
    "#....#...#....",
    "#....#...#..#.",
    "#....#...#..#.",
    "#....#...#..#.",
    "##############",
    "..............",
    "..............",
    "..............",
]);

/// `i`'s mark.
pub fn mark(i: Instrument) -> &'static Mark {
    match i {
        Instrument::Kick => &KICK,
        Instrument::Snare => &SNARE,
        Instrument::Tom => &TOM,
        Instrument::Clap => &CLAP,
        Instrument::Cowbell => &COWBELL,
        Instrument::Cymbal => &CYMBAL,
        Instrument::Hat => &HAT,
        Instrument::Jingle => &JINGLE,
        Instrument::Shaker => &SHAKER,
        Instrument::Sine => &SINE,
        Instrument::Noise => &NOISE,
        Instrument::Impulse => &IMPULSE,
    }
}

/// A family's 3 x 5 letter, three bits a row, pixel `x` at bit `x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Letter([u8; 5]);

impl Letter {
    const fn of(rows: [&str; 5]) -> Letter {
        let mut l = [0; 5];
        let mut y = 0;
        while y < rows.len() {
            let b = rows[y].as_bytes();
            assert!(b.len() == 3, "a letter row is 3 px");
            let mut x = 0;
            while x < b.len() {
                l[y] |= ((b[x] == b'#') as u8) << x;
                x += 1;
            }
            y += 1;
        }
        Letter(l)
    }

    pub fn at(&self, x: i32, y: i32) -> bool {
        self.0[y as usize] >> x & 1 == 1
    }
}

// The families' badges: the mockup's 3 x 5 letters.
static T: Letter = Letter::of(["###", ".#.", ".#.", ".#.", ".#."]);
static M: Letter = Letter::of(["#.#", "###", "#.#", "#.#", "#.#"]);
static B: Letter = Letter::of(["##.", "#.#", "##.", "#.#", "##."]);
static P: Letter = Letter::of(["##.", "#.#", "##.", "#..", "#.."]);

/// `f`'s badge.
pub fn letter(f: Family) -> &'static Letter {
    match f {
        Family::Tone => &T,
        Family::Metal => &M,
        Family::Body => &B,
        Family::Plain => &P,
    }
}

/// A mark's second tone under `ink`: the theme token nearest 40 % of it
/// over the background (light mode swaps tokens, never blends).
fn dim(ink: Rgb565) -> Rgb565 {
    match ink {
        theme::INK => theme::BAR_REST,
        theme::ACCENT => theme::ACCENT_SOFT,
        _ => theme::PILL_EDGE,
    }
}

/// An icon's width and height: the badge's column, a gap, the mark.
pub const ICON_W: i32 = 6 + MARK_W;
pub const ICON_H: i32 = MARK_H;

/// The badge's left column in an icon.
pub const BADGE_X: i32 = 1;

/// An icon's pixel size, in screen pixels: whole, so it stays crisp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scale(i32);

impl Scale {
    /// The band's pads.
    pub const ONE: Scale = Scale(1);
    /// The focus band's: the largest that fits the gauge's 48 px.
    pub const FOCUS: Scale = Scale(4);

    pub const fn px(self) -> i32 {
        self.0
    }
}

const _: () = assert!(ICON_H * Scale::FOCUS.0 <= theme::WAVE_H);

/// What inks an icon pixel.
#[derive(Clone, Copy)]
enum Ink {
    Mark(Tone),
    Badge,
}

/// `m`'s icon's inked pixels from its top left: its family's letter low
/// on the left, its instrument right of it.
fn inks(m: Machine) -> impl Iterator<Item = (i32, i32, Ink)> {
    let (mk, l) = (mark(instrument(m)), letter(family(m)));
    let marks = (0..MARK_H * MARK_W).filter_map(move |i| {
        let (px, py) = (i % MARK_W, i / MARK_W);
        let t = mk.at(px, py);
        (t != Tone::None).then_some((6 + px, py, Ink::Mark(t)))
    });
    let letters = (0..15).filter_map(move |i| {
        let (px, py) = (i % 3, i / 3);
        l.at(px, py).then_some((BADGE_X + px, 7 + py, Ink::Badge))
    });
    marks.chain(letters)
}

/// Machine `m`'s icon from `(x, y)`, `ICON_W` x `ICON_H` at `s`: the mark
/// in `ink` and its dim tone, the badge in `badge`.
pub fn icon<D>(d: &mut D, m: Machine, x: i32, y: i32, s: Scale, ink: Rgb565, badge: Rgb565)
where
    D: DrawTarget<Color = Rgb565>,
{
    let soft = dim(ink);
    let colour = |k| match k {
        Ink::Mark(Tone::Dim) => soft,
        Ink::Mark(_) => ink,
        Ink::Badge => badge,
    };
    let n = s.px();
    for (px, py, k) in inks(m) {
        draw::fill_rect(d, x + px * n, y + py * n, n, n, colour(k));
    }
}

/// Machine `m`'s icon large at the focus band's right, its bottom on the
/// value's baseline.
pub fn big_icon<D>(d: &mut D, m: Machine)
where
    D: DrawTarget<Color = Rgb565>,
{
    icon(d, m, BIG_X, BIG_Y, Scale::FOCUS, theme::ACCENT, theme::INK2);
}

/// The large icon's top left.
const BIG_X: i32 = theme::SCREEN_W - theme::MARGIN_X - ICON_W * Scale::FOCUS.0;
const BIG_Y: i32 = theme::WAVE_Y;

/// The large icon's leftmost inked column: its badge's.
pub const BIG_LEFT: i32 = BIG_X + BADGE_X * Scale::FOCUS.0;

/// What the band draws: the kit, the selected pad, the lit ones.
#[derive(Clone, Copy, Debug)]
pub struct Pads<'a> {
    pub kit: &'a Kit,
    pub selected: PadId,
    pub lit: PadSet,
}

const COLS: i32 = 6;
const PAD_W: i32 = 34;
const PAD_H: i32 = 30;
const PITCH_X: i32 = 36;
const PITCH_Y: i32 = 32;
const TOP: i32 = theme::VIZ_BAND_TOP + 3;
const _: () = assert!(theme::VIZ_LEFT + (COLS - 1) * PITCH_X + PAD_W <= theme::VIZ_RIGHT);
const _: () = assert!(TOP + PITCH_Y + PAD_H <= theme::VIZ_BAND_BOTTOM);

/// A choke group's letter; OFF none.
fn choke_letter(c: ChokeGroup) -> &'static str {
    match c {
        ChokeGroup::Off => "",
        ChokeGroup::A => "A",
        ChokeGroup::B => "B",
        ChokeGroup::C => "C",
        ChokeGroup::D => "D",
    }
}

/// The twelve pads, two rows of six: the number top left, the choke
/// group top right, the icon under them.
pub fn draw<D>(d: &mut D, p: &Pads)
where
    D: DrawTarget<Color = Rgb565>,
{
    for n in PadId::ALL {
        let i = n.index() as i32;
        let (x, y) = (
            theme::VIZ_LEFT + i % COLS * PITCH_X,
            TOP + i / COLS * PITCH_Y,
        );
        let (sel, lit) = (n == p.selected, p.lit.contains(n));
        if lit {
            draw::round_rect(d, x, y, PAD_W, PAD_H, 3, theme::ACCENT_SOFT);
        }
        let edge = if sel { theme::ACCENT } else { theme::PILL_EDGE };
        draw::round_outline(d, x, y, PAD_W, PAD_H, 3, edge);
        let ink = match (lit, sel) {
            (true, _) => theme::INK,
            (false, true) => theme::ACCENT,
            (false, false) => theme::MID,
        };
        let number = crate::ui::view::PAD_NUMBERS[n.index()];
        draw::text(d, &theme::FONT_LABEL, number, x + 4, y + 10, ink);
        let pad = p.kit.pad(n);
        let c = choke_letter(pad.choke());
        draw::text_right(
            d,
            &theme::FONT_LABEL,
            c,
            x + PAD_W - 4,
            y + 10,
            theme::INK2,
            0,
        );
        let badge = if lit { theme::INK } else { theme::INK2 };
        icon(d, pad.machine(), x + 7, y + 14, Scale::ONE, ink, badge);
    }
}

/// What the band is drawn from, for its region's key.
pub fn key(p: &Pads) -> u32 {
    let pads = PadId::ALL.map(|n| {
        let pad = p.kit.pad(n);
        u32::from(pad.machine().code()) | (pad.choke() as u32) << 8
    });
    region::fold_keys(
        u64::from(p.lit.bits()) | (p.selected.index() as u64) << 16,
        &pads,
    )
}
