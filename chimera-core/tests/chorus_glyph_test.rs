//! The chorus's FLANGER and PHASER glyphs (ADR 0323): a moiré of two
//! combs and a ray gun, in the braid's box. Chorus I, II and I+II keep
//! the braid.

mod screen;

use chimera_core::dsp::chorus::ChorusMode;
use chimera_core::ui::components::draw_gauge;
use chimera_core::ui::glyph::{Braid, BraidPart, Gauge};
use chimera_core::ui::notes::chorus::{self, Gun, Picture};
use chimera_core::ui::theme;
use embedded_graphics::pixelcolor::Rgb565;
use screen::Fb;

const MODES: [ChorusMode; 6] = [
    ChorusMode::Off,
    ChorusMode::JunoI,
    ChorusMode::JunoII,
    ChorusMode::JunoBoth,
    ChorusMode::Flanger,
    ChorusMode::Phaser,
];
const OURS: [ChorusMode; 2] = [ChorusMode::Flanger, ChorusMode::Phaser];

/// The box's left column for the pictures (2 px in) and its top.
const X0: i32 = theme::BRAID_X + 2;
const Y0: i32 = theme::BRAID_Y;

/// `mode` at RATE, DEPTH and MIX, `focus`, at `frame`.
fn at(mode: ChorusMode, set: [f32; 3], focus: Option<BraidPart>, frame: u32) -> Braid {
    Braid {
        mode,
        ..Braid::from_set([0.0, set[0], set[1], set[2]], focus, frame)
    }
}

fn shot(b: Braid) -> Fb {
    let mut fb = Fb::new();
    draw_gauge(&mut fb, Gauge::Braid(b));
    fb
}

/// Pixels of `c` in screen columns `xs`, rows `ys`.
fn count_in(fb: &Fb, xs: core::ops::Range<i32>, ys: core::ops::Range<i32>, c: Rgb565) -> usize {
    xs.flat_map(|x| ys.clone().map(move |y| (x, y)))
        .filter(|&(x, y)| fb.at(x, y) == c)
        .count()
}

/// Pixels of `c` in the whole box.
fn count(fb: &Fb, c: Rgb565) -> usize {
    count_in(
        fb,
        theme::BRAID_X..theme::BRAID_X + theme::BRAID_W,
        Y0..Y0 + theme::BRAID_H,
        c,
    )
}

/// Box-relative `(x, y, w, h)` as screen ranges.
fn area((x, y, w, h): (i32, i32, i32, i32)) -> (core::ops::Range<i32>, core::ops::Range<i32>) {
    (X0 + x..X0 + x + w, Y0 + y..Y0 + y + h)
}

#[test]
fn each_mode_has_its_picture() {
    for m in MODES {
        let want = match m {
            ChorusMode::Off | ChorusMode::JunoI | ChorusMode::JunoII | ChorusMode::JunoBoth => {
                Picture::Braid
            }
            ChorusMode::Flanger => Picture::Moire,
            ChorusMode::Phaser => Picture::RayGun,
        };
        assert_eq!(chorus::picture(m), want, "{m:?}");
    }
}

#[test]
fn the_braid_modes_draw_the_braid_and_the_others_their_own() {
    let set = [0.5, 0.8, 1.0];
    for frame in [0, 37] {
        let juno = shot(at(ChorusMode::JunoI, set, None, frame));
        let flanger = shot(at(ChorusMode::Flanger, set, None, frame));
        let phaser = shot(at(ChorusMode::Phaser, set, None, frame));
        assert_ne!(flanger.hash(), juno.hash());
        assert_ne!(phaser.hash(), juno.hash());
        assert_ne!(phaser.hash(), flanger.hash());
        // The braid's strands behind the line are FAINT; neither object
        // draws FAINT unfocused at this MIX.
        assert!(count(&juno, theme::FAINT) > 0);
        assert_eq!(count(&flanger, theme::FAINT), 0);
        assert_eq!(count(&phaser, theme::FAINT), 0);
    }
}

#[test]
fn they_move_with_the_clock_and_rate_smoothly() {
    for m in OURS {
        let travel = |rate: f32, frame| chorus::travel(&at(m, [rate, 1.0, 1.0], None, frame));
        // How far it went from `a` to `b`, the repeat folded out.
        let moved = |rate: f32, a, b| {
            let ((x0, _), (x1, period)) = (travel(rate, a), travel(rate, b));
            (x1 - x0).rem_euclid(period)
        };
        assert!(moved(0.5, 0, 3) > 0.5, "{m:?} moves with the clock");
        assert!(
            moved(1.0, 0, 2) > 2.0 * moved(0.0, 0, 2),
            "{m:?} RATE speeds it"
        );
        // Smooth at full RATE: never more than 5 px a frame, never back.
        for f in 0..400 {
            let step = moved(1.0, f, f + 1);
            let (_, period) = travel(1.0, f + 1);
            assert!(
                step <= 5.0 && step < period - 0.5,
                "{m:?} frame {f}: {step} px"
            );
        }
    }
}

#[test]
fn depth_stretches_the_front_comb_and_grows_the_gun() {
    let span = |depth: f32| {
        let ps: Vec<f32> = (0..300)
            .map(|f| chorus::pitch(&at(ChorusMode::Flanger, [1.0, depth, 1.0], None, f)))
            .collect();
        ps.iter().copied().fold(f32::MIN, f32::max) - ps.iter().copied().fold(f32::MAX, f32::min)
    };
    assert_eq!(span(0.0), 0.0);
    assert!(span(1.0) > 1.5 && span(1.0) > 2.0 * span(0.3));
    // A skinny pistol to a wide boxy blaster, and a fatter beam.
    let (pistol, blaster) = (Gun::at(0.0), Gun::at(1.0));
    assert!(blaster.body.2 > pistol.body.2 && blaster.body.3 > 2 * pistol.body.3);
    assert!(blaster.beam.3 > 3 * pistol.beam.3);
    let accent = |depth| {
        count(
            &shot(at(ChorusMode::Phaser, [0.5, depth, 1.0], None, 4)),
            theme::ACCENT,
        )
    };
    assert!(accent(1.0) > accent(0.0));
}

#[test]
fn mix_zero_leaves_the_back_comb_and_the_gun() {
    for frame in [0, 11, 90] {
        // FLANGER: the back comb alone.
        let fb = shot(at(ChorusMode::Flanger, [0.7, 1.0, 0.0], None, frame));
        assert!(count(&fb, theme::MID) > 0);
        assert_eq!(count(&fb, theme::ACCENT) + count(&fb, theme::INK2), 0);
        let lit = shot(at(ChorusMode::Flanger, [0.7, 1.0, 1.0], None, frame));
        assert!(
            count(&lit, theme::INK2) > 0,
            "bright where the bars line up"
        );
        // PHASER: the gun, its beam off.
        for depth in [0.0, 1.0] {
            let g = Gun::at(depth);
            let (xs, ys) = area(g.beam);
            let (xs, ys) = (xs.start..xs.end, ys.start - 1..ys.end + 1);
            let dark = shot(at(ChorusMode::Phaser, [0.7, depth, 0.0], None, frame));
            assert_eq!(count_in(&dark, xs.clone(), ys.clone(), theme::BG), 0);
            let total: usize = [
                theme::FAINT,
                theme::MID,
                theme::ACCENT,
                theme::INK2,
                theme::INK,
            ]
            .into_iter()
            .map(|c| count_in(&dark, xs.clone(), ys.clone(), c))
            .sum();
            assert_eq!(total, 0, "depth {depth}: no beam");
            assert!(count(&dark, theme::ACCENT) > 0, "the gun");
            let on = shot(at(ChorusMode::Phaser, [0.7, depth, 1.0], None, frame));
            assert!(count_in(&on, xs, ys, theme::INK2) > 0, "pulses at MIX 1");
        }
    }
}

#[test]
fn the_focused_part_lights_as_the_braids_do() {
    let set = [0.5, 0.6, 0.8];
    let frame = 23;
    let dot = (
        X0 + chorus::MODE_DOT.0 - 2..X0 + chorus::MODE_DOT.0 + 3,
        Y0 + chorus::MODE_DOT.1 - 2..Y0 + chorus::MODE_DOT.1 + 3,
    );
    for m in OURS {
        let fb = |focus| shot(at(m, set, focus, frame));
        assert_eq!(count(&fb(None), theme::INK), 0, "{m:?}: no INK unfocused");
        // MODE: a dot under the left, nothing else.
        let f = fb(Some(BraidPart::Mode));
        let d = count_in(&f, dot.0.clone(), dot.1.clone(), theme::INK);
        assert!(d >= 9 && d == count(&f, theme::INK), "{m:?} MODE");
    }
    // FLANGER: RATE the front comb (it slides), DEPTH the bands where the
    // bars line up (it stretches the pitch that beats), MIX the back comb.
    let fl = |focus| shot(at(ChorusMode::Flanger, set, focus, frame));
    let rows = |(t, b): (i32, i32)| Y0 + t..Y0 + b;
    let front = rows((9, 41));
    let back_only = Y0 + 4..Y0 + 9;
    let cols = X0..X0 + 76;
    let rate = fl(Some(BraidPart::Rate));
    assert!(count_in(&rate, cols.clone(), front.clone(), theme::INK) > 100);
    assert_eq!(
        count_in(&rate, cols.clone(), back_only.clone(), theme::INK),
        0
    );
    let depth = fl(Some(BraidPart::Depth));
    let none = fl(None);
    assert_eq!(
        count(&depth, theme::INK),
        count(&none, theme::INK2),
        "the bands lit"
    );
    assert!(count(&depth, theme::FAINT) > 0);
    let mix = fl(Some(BraidPart::Mix));
    assert_eq!(
        count_in(&mix, cols.clone(), back_only.clone(), theme::INK),
        count_in(&none, cols, back_only, theme::MID)
    );
    // PHASER: RATE the pulses, DEPTH the gun's outline, MIX the beam.
    let g = Gun::at(set[1]);
    let ph = |focus| shot(at(ChorusMode::Phaser, set, focus, frame));
    let (bxs, bys) = area(g.beam);
    let beam_rows = bys.start - 1..bys.end + 1;
    let rate = ph(Some(BraidPart::Rate));
    let lit = count_in(&rate, bxs.clone(), beam_rows.clone(), theme::INK);
    assert!(
        lit > 0 && lit == count(&rate, theme::INK),
        "PHASER RATE: the pulses"
    );
    let depth = ph(Some(BraidPart::Depth));
    let (gxs, gys) = area(g.body);
    assert!(
        count_in(&depth, gxs.clone(), gys.clone(), theme::INK) >= (2 * (g.body.2 + g.body.3) - 4) as usize
    );
    assert_eq!(
        count_in(&depth, bxs.clone(), beam_rows.clone(), theme::INK),
        0
    );
    let mix = ph(Some(BraidPart::Mix));
    assert!(
        count_in(&mix, bxs.clone(), bys.clone(), theme::INK) > 0,
        "PHASER MIX: the beam"
    );
    assert_eq!(count_in(&mix, gxs, gys, theme::INK), 0);
}

#[test]
fn the_pictures_stay_in_the_box() {
    let (x, y, w, h) = (
        theme::BRAID_X,
        theme::BRAID_Y,
        theme::BRAID_W,
        theme::BRAID_H,
    );
    for m in OURS {
        for v in [0.0, 0.3, 0.5, 1.0] {
            for frame in (0..300).step_by(7) {
                for focus in BraidPart::ALL.map(Some).into_iter().chain([None]) {
                    let fb = shot(at(m, [v, 1.0 - v, v], focus, frame));
                    assert_eq!(fb.oob, 0);
                    for py in 0..320 {
                        for px in 0..240 {
                            if fb.px[py as usize * 240 + px as usize] != 0 {
                                assert!(
                                    (x..x + w).contains(&px) && (y..y + h).contains(&py),
                                    "{m:?} at ({px}, {py})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
