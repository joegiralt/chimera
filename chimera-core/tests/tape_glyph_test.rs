//! TAPE's machine glyphs (#641, ADR 0148): each machine's still and live
//! pictures locked by golden, the focus band's glyph keyed only on its
//! quantised steps (a dirty redraw is a full one while a knob turns, and
//! nothing redraws at idle).
//!
//! `TAPE_GLYPH_DUMP=<dir>` writes each case at 3× to `<dir>/<case>.ppm`.

// The raster is TAPE's DSP's: a build without it draws no glyph.
#![cfg(feature = "tape")]

mod common;
mod screen;

use chimera_core::dsp::tape_engine::{PlayMode, TapeModel};
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::components::TAPE_GAP;
use chimera_core::ui::perf::PerfStats;
use chimera_core::ui::tape_glyph::{
    self, Bellows, Blind, Film, H, Kind, Loop, Machine, Palette, Q5, Q6, Run, Sponge, TapeGauge, W,
    Wear,
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
            tape: chimera_core::ui::tape_glyph::Q::of(tape),
            age: chimera_core::ui::tape_glyph::Q::of(age),
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
        // PING: wide and level, narrow and steep.
        ("ping_wide", Machine::Ping(loop_(0.5, 1.0, 0.0))),
        ("ping_narrow_steep", Machine::Ping(loop_(0.1, 0.1, -1.0))),
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
        // STRETCH: frozen, natural, squeezed, backward.
        (
            "stretch_frozen",
            Machine::Stretch(Bellows {
                speed: bi6(0.0),
                d_time: q5(0.0),
            }),
        ),
        (
            "stretch_natural",
            Machine::Stretch(Bellows {
                speed: bi6(0.5),
                d_time: q5(0.5),
            }),
        ),
        (
            "stretch_squeezed",
            Machine::Stretch(Bellows {
                speed: bi6(1.0),
                d_time: q5(1.0),
            }),
        ),
        (
            "stretch_backward",
            Machine::Stretch(Bellows {
                speed: bi6(-1.0),
                d_time: q5(0.3),
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
            Machine::Scrub(Sponge {
                finger: q6(0.2),
                speed: bi6(0.0),
                length: q5(0.2),
                drag: q5(0.0),
            }),
        ),
        (
            "scrub_forward",
            Machine::Scrub(Sponge {
                finger: q6(0.8),
                speed: bi6(1.0),
                length: q5(0.6),
                drag: q5(1.0),
            }),
        ),
        (
            "scrub_back",
            Machine::Scrub(Sponge {
                finger: q6(0.4),
                speed: bi6(-0.5),
                length: q5(0.0),
                drag: q5(0.5),
            }),
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

const GOLDENS: &[(&str, u64)] = &[
    ("one_still", 0xd21fca260fc72bb3),
    ("loop_still", 0x008b612ee4dda4c5),
    ("ping_still", 0x11fb45bbcaa0aaad),
    ("tron_still", 0x2da91fd3c252b178),
    ("stretch_still", 0xb9668e1e280b56e7),
    ("wave_still", 0x35b91a1f3869788b),
    ("scrub_still", 0x45ae525d75eda6ef),
    ("one_whole", 0x5fb8d1a1a37be6ee),
    ("one_late", 0x9ce01850f1516906),
    ("one_backward", 0x13f41d858cccb371),
    ("loop_sits", 0x16d9f8c953d2707a),
    ("loop_runs_right", 0xe00cb06a9b6f8608),
    ("loop_runs_left", 0x86e6d55362637b24),
    ("ping_wide", 0xd208957ef175a305),
    ("ping_narrow_steep", 0x6631576472a91c5d),
    ("tron_short_fast", 0xe4b7d0c60805eab3),
    ("tron_long_slow", 0x829ed9e2a1494395),
    ("stretch_frozen", 0x2cd7b98c0971ce88),
    ("stretch_natural", 0xd28bb4170d4e09f1),
    ("stretch_squeezed", 0x84b7d78ba99fe847),
    ("stretch_backward", 0xb8d3f9eaf28f1ea5),
    ("wave_first", 0x58ef7d42209aa44c),
    ("wave_scan_fast", 0xe07365d25ef85b35),
    ("scrub_standstill", 0x7fc38edb009b261f),
    ("scrub_forward", 0x839b130ec71642fb),
    ("scrub_back", 0x7de72b3b2be4b3c6),
    ("loop_wow", 0x0e3fc4752d88f891),
    ("loop_grain", 0xc8c03e9ba5564a58),
    ("tron_worn", 0xa028652aeee8bb7d),
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
    let at =
        |v: f32| TapeGauge::from_set([0.0, 0.5, 0.0, 1.0, v, 0.5, 0.5, 0.3, 0.0, 0.0], false).key();
    let step = 1.0 / Q6::MAX as f32;
    assert_eq!(at(0.5), at(0.5 + 0.4 * step));
    assert_ne!(at(0.5), at(0.5 + step));
}

#[test]
fn model_and_mode_pick_the_glyph() {
    let set = |model: TapeModel, mode: PlayMode| {
        let m = model.index() as f32 / 4.0;
        let p = mode as u8 as f32 / 2.0;
        TapeGauge::from_set([m, p, 0.0, 1.0, 0.3, 0.6, 0.5, 0.3, 0.0, 0.0], false).kind()
    };
    assert_eq!(set(TapeModel::Play, PlayMode::One), Kind::One);
    assert_eq!(set(TapeModel::Play, PlayMode::Loop), Kind::Loop);
    assert_eq!(set(TapeModel::Play, PlayMode::Ping), Kind::Ping);
    assert_eq!(set(TapeModel::Tron, PlayMode::Ping), Kind::Tron);
    assert_eq!(set(TapeModel::Scrub, PlayMode::Loop), Kind::Scrub);
    assert_eq!(set(TapeModel::Stretch, PlayMode::Loop), Kind::Stretch);
    assert_eq!(set(TapeModel::Transwave, PlayMode::One), Kind::Wave);
    // Still: the knobs don't move it.
    let a = TapeGauge::from_set([0.25, 0.5, 0.0, 1.0, 0.1, 0.9, 0.2, 0.3, 1.0, 1.0], true);
    let b = TapeGauge::from_set([0.25, 0.5, 0.3, 0.4, 0.8, 0.1, 0.9, 0.7, 0.0, 0.0], true);
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
    for model in 0..5i8 {
        let mut ui = chimera_core::ui::UiState::new();
        tape_deck(&mut ui, model);
        settle(&mut ui);
        let perf = PerfStats::zero();
        let mut fb = Fb::new();
        ui.render_dirty_with_scope(&mut fb, &perf, &scope_fixture());
        // MODEL's cell: the still glyph.
        assert!(glyph_box_lit(&fb) > 40, "model {model}: a still glyph");
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
                    "model {model} {enc:?}: dirty frame matches a full render"
                );
            }
        }
        assert!(glyph_box_lit(&fb) > 40, "model {model}: a live glyph");
    }
}

#[test]
fn nothing_redraws_at_idle() {
    for model in 0..5i8 {
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
            assert!(
                flushed.is_empty(),
                "model {model} frame {frame}: {flushed:?}"
            );
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
