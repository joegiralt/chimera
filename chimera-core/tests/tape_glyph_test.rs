//! TAPE's machine glyphs (#641, ADR 0173): each machine's still and live
//! pictures locked by golden, the focus band's glyph keyed only on its
//! quantised steps (a dirty redraw is a full one while a knob turns, and
//! nothing redraws at idle).
//!
//! `TAPE_GLYPH_DUMP=<dir>` writes each case at 3× to `<dir>/<case>.ppm`.

// The raster is TAPE's DSP's: a build without it draws no glyph.
#![cfg(feature = "tape")]

mod common;
mod screen;

use chimera_core::dsp::tape_engine::Way;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::components::TAPE_GAP;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::tape_glyph::{
    self, Bellows, Blind, Bounce, Film, GlyphInputs, H, Kind, Loop, Machine, Palette, Pose, Q, Q3,
    Q4, Q5, Q6, Run, Sponge, TapeGauge, W, Wear,
};
use chimera_core::ui::{draw, theme};
use chimera_hal::EncoderId;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics::prelude::*;
use screen::*;

/// The 64 × 48 box alone, with a margin round it: a pixel drawn outside
/// the box lands in the margin and fails.
struct Box64 {
    px: Vec<u16>,
}

const M: i32 = 8;
const BW: i32 = W + 2 * M;
const BH: i32 = H + 2 * M;

impl Box64 {
    fn new() -> Self {
        let bg = RawU16::from(theme::BG).into_inner();
        Self {
            px: vec![bg; (BW * BH) as usize],
        }
    }

    fn of(g: &TapeGauge) -> Self {
        let mut b = Self::new();
        tape_glyph::draw_at(&mut b, g, &Palette::THEME, M, M);
        b
    }

    fn at(&self, x: i32, y: i32) -> u16 {
        self.px[(y * BW + x) as usize]
    }

    fn outside(&self) -> usize {
        let bg = RawU16::from(theme::BG).into_inner();
        (0..BH)
            .flat_map(|y| (0..BW).map(move |x| (x, y)))
            .filter(|&(x, y)| !(M..M + W).contains(&x) || !(M..M + H).contains(&y))
            .filter(|&(x, y)| self.at(x, y) != bg)
            .count()
    }

    fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for y in M..M + H {
            for x in M..M + W {
                for b in self.at(x, y).to_le_bytes() {
                    h ^= b as u64;
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        h
    }

    /// `$TAPE_GLYPH_DUMP/<name>.ppm` at 3×, the box only.
    fn dump(&self, name: &str) {
        let Some(dir) = std::env::var_os("TAPE_GLYPH_DUMP") else {
            return;
        };
        const S: i32 = 3;
        let mut out = format!("P6\n{} {}\n255\n", W * S, H * S).into_bytes();
        for y in 0..H * S {
            for x in 0..W * S {
                let c: Rgb565 = RawU16::new(self.at(M + x / S, M + y / S)).into();
                out.extend([
                    (c.r() << 3) | (c.r() >> 2),
                    (c.g() << 2) | (c.g() >> 4),
                    (c.b() << 3) | (c.b() >> 2),
                ]);
            }
        }
        let path = std::path::Path::new(&dir).join(format!("{name}.ppm"));
        std::fs::write(&path, out).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }
}

impl OriginDimensions for Box64 {
    fn size(&self) -> Size {
        Size::new(BW as u32, BH as u32)
    }
}

impl DrawTarget for Box64 {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        it: I,
    ) -> Result<(), Self::Error> {
        for Pixel(p, c) in it {
            assert!((0..BW).contains(&p.x) && (0..BH).contains(&p.y), "{p:?}");
            self.px[(p.y * BW + p.x) as usize] = RawU16::from(c).into_inner();
        }
        Ok(())
    }
}

fn q6(v: f32) -> Q6 {
    Q6::of(v)
}
fn q5(v: f32) -> Q5 {
    Q5::of(v)
}
/// A bipolar value's step.
fn bi6(v: f32) -> Q6 {
    Q6::of(0.5 + 0.5 * v)
}
fn bi5(v: f32) -> Q5 {
    Q5::of(0.5 + 0.5 * v)
}

/// A sponge: FINGER, SPEED (bipolar), LENGTH, DRAG, XFADE.
fn sponge(finger: f32, speed: f32, length: f32, drag: f32, xfade: f32) -> Sponge {
    Sponge {
        finger: q6(finger),
        speed: Q4::of(0.5 + 0.5 * speed),
        length: q5(length),
        drag: Q4::of(drag),
        xfade: Q3::of(xfade),
    }
}

fn live(machine: Machine) -> TapeGauge {
    TapeGauge {
        machine,
        wear: Wear::NONE,
    }
}

fn worn(machine: Machine, tape: f32, age: f32) -> TapeGauge {
    TapeGauge {
        machine,
        wear: Wear {
            tape: Q::of(tape),
            age: Q::of(age),
        },
    }
}

fn name(k: Kind) -> &'static str {
    match k {
        Kind::One => "one",
        Kind::Loop => "loop",
        Kind::Ping => "ping",
        Kind::Tron => "tron",
        Kind::Stretch => "stretch",
        Kind::Wave => "wave",
        Kind::Scrub => "scrub",
    }
}

/// Every case: each machine still, then live at representative knobs.
fn cases() -> Vec<(String, TapeGauge)> {
    let mut v: Vec<(String, TapeGauge)> = Kind::ALL
        .iter()
        .map(|&k| (format!("{}_still", name(k)), TapeGauge::still(k)))
        .collect();
    let loop_ = |p, l, w| Loop {
        position: q6(p),
        length: q6(l),
        walk: bi6(w),
        xfade: Q3::of(0.0),
    };
    let bounce = |p, l, w| Bounce {
        position: q6(p),
        length: q6(l),
        walk: bi6(w),
    };
    let live_cases: Vec<(&str, Machine)> = vec![
        // ONE: the whole window, a short run late, and played backward.
        (
            "one_whole",
            Machine::One(Run {
                start: q6(0.0),
                end: q6(1.0),
            }),
        ),
        (
            "one_late",
            Machine::One(Run {
                start: q6(0.7),
                end: q6(0.85),
            }),
        ),
        (
            "one_backward",
            Machine::One(Run {
                start: q6(0.8),
                end: q6(0.2),
            }),
        ),
        // LOOP: sitting, running right with a big wheel, running left small.
        ("loop_sits", Machine::Loop(loop_(0.0, 1.0, 0.0))),
        ("loop_runs_right", Machine::Loop(loop_(0.2, 0.9, 1.0))),
        ("loop_runs_left", Machine::Loop(loop_(0.9, 0.1, -0.6))),
        // POSITION rolls the wheel even at LENGTH 1 (#645); XFADE dashes
        // the loop's end.
        ("loop_whole_right", Machine::Loop(loop_(1.0, 1.0, 0.0))),
        (
            "loop_xfade",
            Machine::Loop(Loop {
                xfade: Q3::of(1.0),
                ..loop_(0.4, 0.6, 0.0)
            }),
        ),
        // PING: wide and level, narrow and steep.
        ("ping_wide", Machine::Ping(bounce(0.5, 1.0, 0.0))),
        ("ping_narrow_steep", Machine::Ping(bounce(0.1, 0.1, -1.0))),
        // TRON: shortest and fast, longest and slow.
        (
            "tron_short_fast",
            Machine::Tron(Blind {
                length: q6(0.0),
                fade: q5(0.0),
            }),
        ),
        (
            "tron_long_slow",
            Machine::Tron(Blind {
                length: q6(1.0),
                fade: q5(1.0),
            }),
        ),
        // STRETCH: CLASSIC frozen and natural, GRAIN squeezed and backward.
        (
            "stretch_frozen",
            Machine::Stretch(Bellows {
                speed: bi6(0.0),
                fold: q5(0.0),
                at: q5(0.0),
                grain: false,
            }),
        ),
        (
            "stretch_natural",
            Machine::Stretch(Bellows {
                speed: bi6(0.5),
                fold: q5(0.5),
                at: q5(0.3),
                grain: false,
            }),
        ),
        (
            "stretch_squeezed",
            Machine::Stretch(Bellows {
                speed: bi6(1.0),
                fold: q5(1.0),
                at: q5(0.0),
                grain: true,
            }),
        ),
        (
            "stretch_backward",
            Machine::Stretch(Bellows {
                speed: bi6(-1.0),
                fold: q5(0.3),
                at: q5(0.0),
                grain: true,
            }),
        ),
        // TRANSWAVE: first frame unscanned, mid-table scanned fast.
        (
            "wave_first",
            Machine::Wave(Film {
                frame: q6(0.0),
                range: q6(0.0),
                scan: bi5(0.0),
            }),
        ),
        (
            "wave_scan_fast",
            Machine::Wave(Film {
                frame: q6(0.53),
                range: q6(0.3),
                scan: bi5(-1.0),
            }),
        ),
        // SCRUB: still (no bubbles), fast forward with a long trail, back.
        (
            "scrub_standstill",
            Machine::Scrub(sponge(0.2, 0.0, 0.2, 0.0, 0.0)),
        ),
        (
            "scrub_forward",
            Machine::Scrub(sponge(0.8, 1.0, 0.6, 1.0, 0.0)),
        ),
        (
            "scrub_back",
            Machine::Scrub(sponge(0.4, -0.5, 0.0, 0.5, 0.0)),
        ),
        // The owner's DRAG 0, LENGTH 27 (#645), its end crossfaded.
        (
            "scrub_length_xfade",
            Machine::Scrub(sponge(0.3, 0.5, 27.0 / 127.0, 0.0, 1.0)),
        ),
    ];
    v.extend(
        live_cases
            .into_iter()
            .map(|(n, m)| (n.to_string(), live(m))),
    );
    // TAPE's wobble and AGE's grain and dropouts.
    v.push(("loop_wow".into(), worn(Kind::Loop.still(), 1.0, 0.0)));
    v.push(("loop_grain".into(), worn(Kind::Loop.still(), 0.0, 0.4)));
    v.push(("tron_worn".into(), worn(Kind::Tron.still(), 0.6, 1.0)));
    v
}

// The still and worn rows are WALK's footsteps since ADR 0203; the live
// rows of ONE's match, LOOP's wheel, PING's court and TRON's blind are the
// kept pictures', unchanged.
const GOLDENS: &[(&str, u64)] = &[
    ("one_still", 0x9f4b5f3936088066),
    ("loop_still", 0xf81953cb39842fa6),
    ("ping_still", 0xe2e893a20f5c771f),
    ("tron_still", 0xc8dfe3dd85713df6),
    ("stretch_still", 0xec7471660def4d69),
    ("wave_still", 0x35b91a1f3869788b),
    ("scrub_still", 0xfa9dc37b17460909),
    ("one_whole", 0x5fb8d1a1a37be6ee),
    ("one_late", 0x9ce01850f1516906),
    ("one_backward", 0x13f41d858cccb371),
    ("loop_sits", 0xbb2e7411805497cb),
    ("loop_runs_right", 0x3dee2f5b8deb3a47),
    ("loop_runs_left", 0x5c2fd12ee536b8fd),
    ("loop_whole_right", 0x830fddb33da0a1dc),
    ("loop_xfade", 0xc42f85f830013f20),
    ("ping_wide", 0xd208957ef175a305),
    ("ping_narrow_steep", 0x6631576472a91c5d),
    ("tron_short_fast", 0xe4b7d0c60805eab3),
    ("tron_long_slow", 0x829ed9e2a1494395),
    ("stretch_frozen", 0x290c61c145a2d408),
    ("stretch_natural", 0xd7fbf6a843d9c0b1),
    ("stretch_squeezed", 0x92840185dae1d62f),
    ("stretch_backward", 0xed3022d8f813ab4d),
    ("wave_first", 0x58ef7d42209aa44c),
    ("wave_scan_fast", 0xe07365d25ef85b35),
    ("scrub_standstill", 0x6324ec910bd1561d),
    ("scrub_forward", 0x823b6cdf60db0701),
    ("scrub_back", 0xde0233cd46fe3318),
    ("scrub_length_xfade", 0x4ae89a8f1af5f0df),
    ("loop_wow", 0x8e80d9f019a555c4),
    ("loop_grain", 0x751dff187a6ad703),
    ("tron_worn", 0xafc96adcb08994d6),
];

#[test]
fn glyphs_match_their_goldens() {
    let got: Vec<(String, u64)> = cases()
        .into_iter()
        .map(|(n, g)| {
            let b = Box64::of(&g);
            b.dump(&n);
            assert_eq!(b.outside(), 0, "{n} drew outside its box");
            (n, b.hash())
        })
        .collect();
    let got: Vec<(&str, u64)> = got.iter().map(|(n, h)| (n.as_str(), *h)).collect();
    common::golden::check(GOLDENS, &got);
}

#[test]
fn every_case_draws_something_and_differs() {
    let hashes: Vec<_> = cases().iter().map(|(_, g)| Box64::of(g).hash()).collect();
    let blank = Box64::new().hash();
    for (i, h) in hashes.iter().enumerate() {
        assert_ne!(*h, blank, "case {i} is blank");
        assert!(!hashes[..i].contains(h), "case {i} repeats an earlier one");
    }
}

#[test]
fn equal_keys_draw_alike_and_moves_change_the_key() {
    // The gauge is its steps (`TapeGauge` is `Copy` and `Eq` over them):
    // equal keys are equal gauges, and drawing is a pure function of one.
    for (n, g) in cases() {
        assert_eq!(Box64::of(&g).hash(), Box64::of(&g).hash(), "{n}");
    }
    let keys: Vec<_> = cases().iter().map(|(_, g)| g.key()).collect();
    for (i, k) in keys.iter().enumerate() {
        assert!(!keys[..i].contains(k), "case {i}'s key repeats");
    }
}

#[test]
fn a_step_is_about_a_pixel() {
    // Below a step nothing changes; the key holds the steps, so a value
    // that moves less than a step costs no redraw.
    let at = |v: f32| {
        TapeGauge::of(&GlyphInputs::from_set(
            [
                0.0,
                0.0,
                1.0,
                v,
                0.5,
                0.5,
                0.3,
                0.0,
                0.0,
                0.2,
                2.0 / 3.0,
                0.0,
                0.0,
                0.5,
                0.0,
            ],
            Pose::Live,
        ))
        .key()
    };
    let step = 1.0 / Q6::MAX as f32;
    assert_eq!(at(0.5), at(0.5 + 0.4 * step));
    assert_ne!(at(0.5), at(0.5 + step));
}

#[test]
fn model_picks_the_glyph() {
    // MODEL's value is the machine, WALK's DIRECTION and TRON its path.
    for w in Way::ALL {
        let (m, d, t) = w.parts();
        let g = TapeGauge::of(&GlyphInputs::from_set(
            [
                m.index() as f32 / 3.0,
                0.0,
                1.0,
                0.3,
                0.6,
                0.5,
                0.3,
                0.0,
                0.0,
                0.2,
                d as u8 as f32 / 3.0,
                t as u8 as f32,
                0.0,
                0.5,
                0.0,
            ],
            Pose::Live,
        ));
        assert_eq!(g.kind(), w);
    }
    // Still: the knobs don't move it.
    let a = TapeGauge::of(&GlyphInputs::from_set(
        [
            0.0, 0.0, 1.0, 0.1, 0.9, 0.2, 0.3, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.5, 0.0,
        ],
        Pose::Still,
    ));
    let b = TapeGauge::of(&GlyphInputs::from_set(
        [
            0.0, 0.3, 0.4, 0.8, 0.1, 0.9, 0.7, 0.0, 0.0, 1.0, 0.0, 1.0, 0.9, 0.5, 0.0,
        ],
        Pose::Still,
    ));
    assert_eq!(a, b);
    assert_eq!(a, TapeGauge::still(Kind::Tron));
}

/// The focus band's rows.
fn band(rows: (u16, u16)) -> bool {
    rows.0 < theme::FOCUS_BOTTOM as u16 && rows.1 > theme::HEADER_BOTTOM as u16 && rows.0 != rows.1
}

fn glyph_box_lit(fb: &Fb) -> usize {
    (theme::WAVE_Y..theme::WAVE_Y + H)
        .flat_map(|y| (theme::WAVE_X..theme::WAVE_X + W).map(move |x| (x, y)))
        .filter(|&(x, y)| fb.at(x, y) != theme::BG)
        .count()
}

#[test]
fn dirty_redraw_equals_full_while_a_knob_turns() {
    for model in Way::ALL {
        let mut ui = chimera_core::ui::UiState::new();
        tape_deck(&mut ui, model);
        settle(&mut ui);
        let perf = PerfStats::zero();
        let mut fb = Fb::new();
        ui.render_dirty_with_scope(&mut fb, &perf, &scope_fixture());
        // MODEL's cell: the still glyph.
        assert!(glyph_box_lit(&fb) > 40, "{model:?}: a still glyph");
        for (enc, by) in [
            (EncoderId::B, 9),
            (EncoderId::C, -7),
            (EncoderId::D, 5),
            (EncoderId::E, 6),
        ] {
            feed(&mut ui, Input::turn(enc, by));
            for _ in 0..12 {
                ui.update(UiTick::for_test());
                ui.render_dirty_with_scope(&mut fb, &perf, &scope_fixture());
                let mut full = Fb::new();
                ui.render_with_scope(&mut full, &perf, &scope_fixture());
                assert!(
                    fb.px == full.px,
                    "{model:?} {enc:?}: dirty frame matches a full render"
                );
            }
        }
        assert!(glyph_box_lit(&fb) > 40, "{model:?}: a live glyph");
    }
}

#[test]
fn nothing_redraws_at_idle() {
    for model in Way::ALL {
        let mut ui = chimera_core::ui::UiState::new();
        tape_deck(&mut ui, model);
        feed(&mut ui, Input::turn(EncoderId::B, 4));
        settle(&mut ui);
        let perf = PerfStats::zero();
        let mut fb = Fb::new();
        ui.render_dirty_with_scope(&mut fb, &perf, &scope_fixture());
        for frame in 0..60 {
            ui.update(UiTick::for_test());
            let flushed: Vec<_> = ui
                .render_dirty_with_scope(&mut fb, &perf, &scope_fixture())
                .into_iter()
                .filter(|&r| band(r))
                .collect();
            assert!(flushed.is_empty(), "{model:?} frame {frame}: {flushed:?}");
        }
    }
}

#[test]
fn values_clear_the_glyph() {
    // Every word a TAPE cell can show sits left of the glyph, at the
    // focus size or the readout's.
    let words = chimera_core::dsp::tape_engine::TAPE_ENGINE_SPECS
        .iter()
        .filter(|s| s.glyph == chimera_core::ui::glyph::FocusGlyph::Tape)
        .flat_map(|s| match s.fmt {
            chimera_core::block::ValFmt::Names(n) => n.to_vec(),
            _ => vec![],
        });
    for w in words {
        let room = theme::WAVE_X - theme::FOCUS_VALUE_X - TAPE_GAP;
        let fits = draw::text_width(&theme::FONT_FOCUS, w, 0) <= room
            || draw::text_width(&theme::FONT_READOUT, w, 0) <= room;
        assert!(fits, "{w}");
    }
}

// ── UAT round 1 (#645) ───────────────────────────────────────────────

fn ink(i: tape_glyph::Ink) -> u16 {
    RawU16::from(Palette::THEME.of(i)).into_inner()
}

fn count(b: &Box64, i: tape_glyph::Ink) -> usize {
    let c = ink(i);
    (M..M + H)
        .flat_map(|y| (M..M + W).map(move |x| (x, y)))
        .filter(|&(x, y)| b.at(x, y) == c)
        .count()
}

#[test]
fn position_rolls_the_wheel_whole_inside_the_box() {
    // POSITION rolled nothing at LENGTH 1, and a 19 px wheel had no room
    // to roll elsewhere. Now it rolls at every LENGTH, and its rims never
    // clip at the ends: the ring's pixels are those of the middle.
    for len in [0.0, 0.5, 0.9, 1.0] {
        for walk in [-1.0, 0.0, 1.0] {
            let at = |pos: f32| {
                Box64::of(&live(Machine::Loop(Loop {
                    position: q6(pos),
                    length: q6(len),
                    walk: bi6(walk),
                    xfade: Q3::of(0.0),
                })))
            };
            let (a, mid, z) = (at(0.0), at(0.5), at(1.0));
            assert_ne!(a.hash(), z.hash(), "len {len} walk {walk}: it rolls");
            let rim = count(&mid, tape_glyph::Ink::Ink2);
            assert_eq!(count(&a, tape_glyph::Ink::Ink2), rim, "len {len}: left end");
            assert_eq!(
                count(&z, tape_glyph::Ink::Ink2),
                rim,
                "len {len}: right end"
            );
        }
    }
}

#[test]
fn scrub_arrow_never_hides() {
    // The backward arrow sat under the sponge. Now SPEED's arrow is drawn
    // over it, whole, at every FINGER, both ways.
    let acc = ink(tape_glyph::Ink::Accent);
    for f in 0..=Q6::MAX {
        let finger = f as f32 / Q6::MAX as f32;
        let sx = (14.0 + finger * 36.0).round() as i32;
        for s in 0..=Q4::MAX {
            let speed = 2.0 * s as f32 / Q4::MAX as f32 - 1.0;
            if speed.abs() < 0.05 {
                continue;
            }
            let d = if speed > 0.0 { 1 } else { -1 };
            for (len, drag, xf) in [(0.0, 0.0, 0.0), (1.0, 1.0, 1.0), (0.5, 0.3, 0.5)] {
                let b = Box64::of(&live(Machine::Scrub(sponge(finger, speed, len, drag, xf))));
                let y = M + 17;
                let head = [
                    (2, -2),
                    (2, -1),
                    (2, 0),
                    (2, 1),
                    (2, 2),
                    (3, -1),
                    (3, 1),
                    (4, 0),
                ];
                for (dx, dy) in head {
                    assert_eq!(b.at(M + sx + d * dx, y + dy), acc, "f {f} speed {speed}");
                }
                for dx in -4..=1 {
                    assert_eq!(b.at(M + sx - d * dx, y), acc, "f {f} speed {speed}: shaft");
                }
                // And no bubble crowds it.
                let ring = ink(tape_glyph::Ink::Ink2);
                for yy in y - 3..=y + 3 {
                    for x in sx - 6..=sx + 6 {
                        assert_ne!(b.at(M + x, yy), ring, "f {f} speed {speed}: a bubble");
                    }
                }
            }
        }
    }
}

#[test]
fn scrub_length_is_outlined() {
    // LENGTH's run reads hollow beside the sponge's accent: its ends
    // whole, its inside open on the tape's centre row.
    let finger = 0.3;
    let sx = (14.0f32 + finger * 36.0).round() as i32;
    let lw = ((0.06 + 0.94 * q5(27.0 / 127.0).unit()) * 30.0).round() as i32;
    let b = Box64::of(&live(Machine::Scrub(sponge(
        finger,
        0.5,
        27.0 / 127.0,
        0.0,
        0.0,
    ))));
    let (acc, bg) = (ink(tape_glyph::Ink::Accent), ink(tape_glyph::Ink::Bg));
    assert_eq!(b.at(M + sx, M + 42), acc);
    assert_eq!(b.at(M + sx + lw - 1, M + 42), acc);
    for x in sx + 1..sx + lw - 1 {
        assert_eq!(b.at(M + x, M + 42), bg, "x {x}");
    }
}

#[test]
fn xfade_dashes_the_loop_end_where_it_plays() {
    let wheel = |xf: f32| {
        Box64::of(&live(Machine::Loop(Loop {
            position: q6(0.4),
            length: q6(0.6),
            walk: bi6(0.0),
            xfade: Q3::of(xf),
        })))
        .hash()
    };
    assert_ne!(wheel(0.0), wheel(1.0));
    let scrub = |xf: f32| Box64::of(&live(Machine::Scrub(sponge(0.5, 0.5, 0.8, 0.0, xf)))).hash();
    assert_ne!(scrub(0.0), scrub(1.0));
    // PING has no crossfade: XFADE (no cell there) leaves it.
    let ping = |xf: f32| {
        TapeGauge::of(&GlyphInputs::from_set(
            [
                0.0, 0.0, 1.0, 0.3, 0.6, 0.5, 0.3, 0.0, 0.0, xf, 1.0, 0.0, 0.0, 0.5, 0.0,
            ],
            Pose::Live,
        ))
    };
    assert_eq!(ping(0.0).kind(), Kind::Ping);
    assert_eq!(ping(0.0).key(), ping(1.0).key());
}

#[test]
fn ignored_params_leave_the_glyph() {
    // A param the machine ignores (`reads`: TRON's POSITION and MOTION,
    // ONE's POSITION, DRIFT, LENGTH and XFADE, PING's XFADE) moves neither
    // the key nor a pixel.
    use chimera_core::dsp::tape_engine::reads;
    use chimera_core::ui::glyph::TAPE_GLYPH_PARAMS;
    let mut checked = 0;
    // WALK reads all its cells on every path: what a path's glyph draws
    // is checked by the other tests.
    for w in Way::ALL {
        let (m, d, t) = w.parts();
        let mut set = [
            0.0, 0.1, 0.9, 0.3, 0.6, 0.5, 0.3, 0.2, 0.1, 0.2, 0.0, 0.0, 0.0, 0.4, 0.0,
        ];
        (set[0], set[10], set[11]) = (m.index() as f32 / 3.0, d as u8 as f32 / 3.0, t as u8 as f32);
        let base = TapeGauge::of(&GlyphInputs::from_set(set, Pose::Live));
        for (i, a) in TAPE_GLYPH_PARAMS.iter().enumerate().skip(1) {
            if reads(m, a.param) {
                continue;
            }
            for v in [0.0, 1.0] {
                let mut moved = set;
                moved[i] = v;
                let g = TapeGauge::of(&GlyphInputs::from_set(moved, Pose::Live));
                assert_eq!(g.key(), base.key(), "{m:?} {:?}", a.param);
                assert_eq!(Box64::of(&g).hash(), Box64::of(&base).hash());
            }
            checked += 1;
        }
    }
    assert!(checked >= 8, "{checked}");
}

#[test]
fn no_tape_page_cell_is_dimmed() {
    // Each machine's pages list only what it reads (ADR 0178), so no
    // TAPE cell reads dimmed and none needs ADR 0173's dimmed glyph, but
    // what can't be heard: WOW at TAPE 0, LOOP's and PING's FADE under an
    // AMP route (tape_musical_ui_test), and STRETCH's other TYPE's cells.
    // Here TAPE runs and no route reaches VCA.
    use chimera_core::ui::block_registry::{TAPE_DECK, TAPE_MODEL};
    use chimera_core::ui::view::{self, SlotCtx};
    for m in Way::ALL {
        let mut s = chimera_core::preset::Sound::init(chimera_core::params::EngineType::Tape);
        s.params.tape.set_way(m);
        s.params.tape.period = 109.1;
        s.params.tape.tape = 0.3;
        s.mod_state = Default::default();
        let ctx = SlotCtx::read(&s.params, chimera_core::addr::Op::A);
        for def in [&TAPE_DECK, &TAPE_MODEL] {
            for i in 0..6 {
                let v = view::view(def, i, &ctx);
                // WALK's XFADE off LOOP and RETRIG off once through say why.
                let why = v.addr().and_then(|a| view::tape_note(a, &s));
                if matches!(
                    why,
                    Some(
                        view::XFADE_LOOP_ONLY
                            | view::XFADE_NO_SEAM
                            | view::RETRIG_ONCE_ONLY
                            | view::RETRIG_TRON_OFF
                    )
                ) {
                    continue;
                }
                // STRETCH dims the other TYPE's cells (ADR 0204), and only
                // those.
                let other = v.addr().is_some_and(|a| {
                    chimera_core::dsp::tape_engine::TapeEngine::stretch_only(
                        s.params.tape.kind,
                        a.param,
                    )
                    .is_some()
                        && m == Way::Stretch
                });
                assert_eq!(
                    view::is_dimmed(&v, &s, &ctx),
                    other,
                    "{m:?} {} {i}",
                    def.short
                );
            }
        }
    }
}

#[test]
fn stretch_chevrons_count_the_speed_not_the_knob() {
    // SPEED runs −1× to +2× (`stretch::time_speed`): full left is 1×
    // back, two chevrons, as 1× forward is; full right, 2×, is four.
    let chevrons = |m: f32| {
        let b = Box64::of(&live(Machine::Stretch(Bellows {
            speed: bi6(m),
            fold: q5(0.5),
            at: q5(0.0),
            grain: true,
        })));
        let acc = ink(tape_glyph::Ink::Accent);
        (M + 42..M + H)
            .flat_map(|y| (M..M + W).map(move |x| (x, y)))
            .filter(|&(x, y)| b.at(x, y) == acc)
            .count()
    };
    // 0.48 is the step just under 1×.
    assert_eq!(chevrons(-1.0), chevrons(0.48));
    assert_eq!(chevrons(1.0), 2 * chevrons(0.48));
}

#[test]
fn direction_turned_onto_forward_burns_the_match() {
    // LOOP → FORWARD → LOOP on DECK, as on the unit (ADR 0203): FORWARD's
    // LENGTH (B) and POSITION (E) cut the match's run; back on LOOP,
    // POSITION rolls the wheel. A dirty frame always equals a full one.

    use chimera_core::project::PartId;
    let perf = PerfStats::zero();
    let tape = |ui: &chimera_core::ui::UiState| ui.project().part(PartId::ALL[0]).sound.params.tape;
    let mut fb = Fb::new();
    let mut shot = |ui: &mut chimera_core::ui::UiState| {
        for _ in 0..120 {
            ui.update(UiTick::for_test());
            ui.render_dirty_with_scope(&mut fb, &perf, &scope_fixture());
        }
        let mut full = Fb::new();
        ui.render_with_scope(&mut full, &perf, &scope_fixture());
        assert!(fb.px == full.px, "a dirty frame is a full one");
        (theme::WAVE_Y..theme::WAVE_Y + H)
            .flat_map(|y| (theme::WAVE_X..theme::WAVE_X + W).map(move |x| (x, y)))
            .map(|(x, y)| full.at(x, y))
            .collect::<Vec<_>>()
    };
    let mut ui = stack_ui();
    tape_deck(&mut ui, Way::Loop);
    shot(&mut ui);
    feed(&mut ui, Input::turn(EncoderId::D, -2));
    assert_eq!(tape(&ui).way(), Way::One);
    for (enc, by) in [(EncoderId::B, -20), (EncoderId::E, 20)] {
        let before = shot(&mut ui);
        feed(&mut ui, Input::turn(enc, by));
        assert_ne!(shot(&mut ui), before, "{enc:?} burns the match");
    }
    feed(&mut ui, Input::turn(EncoderId::D, 2));
    assert_eq!(tape(&ui).way(), Way::Loop);
    let before = shot(&mut ui);
    let pos = tape(&ui).position;
    feed(&mut ui, Input::turn(EncoderId::E, -10));
    assert_ne!(shot(&mut ui), before, "POSITION rolls the wheel in LOOP");
    assert!(tape(&ui).position < pos);
}

/// The glyph's inputs decode TYPE and TRON from their normalised set
/// values into their enums (review of #761, m7): GRAIN is TYPE's top
/// (1.0), which the old `>= 1.5` on the normalised value never met, so
/// GRAIN's bellows drew CLASSIC's.
#[test]
fn glyph_inputs_decode_type_and_tron() {
    use chimera_core::dsp::tape_engine::{StretchType, Tron};
    let at = |kind: f32, tron: f32| {
        let mut set = [0.0; 15];
        (set[11], set[14]) = (tron, kind);
        GlyphInputs::from_set(set, Pose::Live)
    };
    assert_eq!(at(0.0, 0.0).kind, StretchType::Mono);
    assert_eq!(at(0.5, 0.0).kind, StretchType::Poly);
    assert_eq!(at(1.0, 0.0).kind, StretchType::Grain);
    assert_eq!(at(0.0, 1.0).tron, Tron::On);
    assert_eq!(at(0.0, 0.0).tron, Tron::Off);
    // STRETCH under GRAIN draws GRAIN's bellows, under CLASSIC CLASSIC's.
    let mut set = [0.0; 15];
    set[0] = 1.0 / 3.0; // MODEL: STRETCH
    set[14] = 1.0;
    let grain = TapeGauge::of_set(set, Pose::Live);
    set[14] = 0.0;
    assert_ne!(grain, TapeGauge::of_set(set, Pose::Live));
}
