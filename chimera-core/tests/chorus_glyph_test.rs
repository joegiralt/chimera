//! The chorus's FLANGER and PHASER glyphs (ADR 0317): a comb and a few
//! wide dips, what each does to the spectrum, in the braid's box. Chorus
//! I, II and I+II keep the braid.

mod screen;

use chimera_core::dsp::chorus::ChorusMode;
use chimera_core::ui::components::draw_gauge;
use chimera_core::ui::glyph::{Braid, BraidPart, Gauge};
use chimera_core::ui::notes::chorus::{self, Picture};
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

/// Pixels of `c` in columns `xs` of the box.
fn count(fb: &Fb, xs: core::ops::Range<i32>, c: Rgb565) -> usize {
    let ys = theme::BRAID_Y..theme::BRAID_Y + theme::BRAID_H;
    xs.flat_map(|x| ys.clone().map(move |y| (x, y)))
        .filter(|&(x, y)| fb.at(x, y) == c)
        .count()
}

fn whole() -> core::ops::Range<i32> {
    theme::BRAID_X..theme::BRAID_X + theme::BRAID_W
}

#[test]
fn each_mode_has_its_picture() {
    for m in MODES {
        let want = match m {
            ChorusMode::Off | ChorusMode::JunoI | ChorusMode::JunoII | ChorusMode::JunoBoth => {
                Picture::Braid
            }
            ChorusMode::Flanger => Picture::Comb,
            ChorusMode::Phaser => Picture::Dips,
        };
        assert_eq!(chorus::picture(m), want, "{m:?}");
    }
}

#[test]
fn the_braid_modes_draw_the_braid_and_the_others_their_own() {
    let set = [0.5, 0.8, 1.0];
    for frame in [0, 37] {
        let juno = shot(at(ChorusMode::JunoI, set, None, frame)).hash();
        let flanger = shot(at(ChorusMode::Flanger, set, None, frame)).hash();
        let phaser = shot(at(ChorusMode::Phaser, set, None, frame)).hash();
        assert_ne!(flanger, juno);
        assert_ne!(phaser, juno);
        assert_ne!(phaser, flanger);
        // The braid: strands in front in ACCENT, behind in FAINT.
        let fb = shot(at(ChorusMode::JunoBoth, set, None, frame));
        assert!(count(&fb, whole(), theme::FAINT) > 0);
    }
}

#[test]
fn notches_travel_with_the_clock_and_rate() {
    for m in [ChorusMode::Flanger, ChorusMode::Phaser] {
        let lead = |rate: f32, frame| chorus::lead(&at(m, [rate, 1.0, 1.0], None, frame));
        assert_ne!(lead(0.5, 0), lead(0.5, 5), "{m:?} moves with the clock");
        // RATE: further along the sweep in the same frames (both still
        // inside its first quarter).
        let from = lead(0.0, 0);
        assert!(
            (lead(1.0, 2) - from).abs() > (lead(0.0, 2) - from).abs(),
            "{m:?} RATE speeds it"
        );
        // Smooth: a frame at full RATE never moves it further than the
        // braid's crest goes (about 4 px), and never jumps.
        for f in 0..400 {
            let step = (lead(1.0, f + 1) - lead(1.0, f)).abs();
            assert!(step <= 5.0, "{m:?} frame {f}: {step} px");
        }
        // DEPTH 0: no sweep.
        let still = |frame| chorus::lead(&at(m, [1.0, 0.0, 1.0], None, frame));
        assert_eq!(still(0), still(13));
        // DEPTH is how far: the full depth sweeps further.
        let span = |depth: f32| {
            let xs: Vec<f32> = (0..200)
                .map(|f| chorus::lead(&at(m, [1.0, depth, 1.0], None, f)))
                .collect();
            xs.iter().copied().fold(f32::MIN, f32::max)
                - xs.iter().copied().fold(f32::MAX, f32::min)
        };
        assert!(span(1.0) > span(0.3) + 4.0, "{m:?}");
    }
}

#[test]
fn mix_sets_the_cut_and_zero_is_flat() {
    let w = theme::BRAID_W - 4;
    for m in [ChorusMode::Flanger, ChorusMode::Phaser] {
        for frame in [0, 11, 90] {
            let flat = at(m, [0.7, 1.0, 0.0], None, frame);
            assert!((0..w).all(|x| chorus::drop(&flat, x) == 0), "{m:?}");
            let deepest = |mix| {
                let b = at(m, [0.7, 1.0, mix], None, frame);
                (0..w).map(|x| chorus::drop(&b, x)).max().unwrap()
            };
            assert!(deepest(1.0) > deepest(0.4), "{m:?}");
            assert!(deepest(0.4) > 0, "{m:?}");
        }
    }
    // The comb: many narrow notches; the phaser: a few wide dips.
    let notches = |m| {
        let b = at(m, [0.0, 0.0, 1.0], None, 0);
        let half = chorus::drop(&b, 0).max((0..w).map(|x| chorus::drop(&b, x)).max().unwrap()) / 2;
        let cut: Vec<bool> = (0..w).map(|x| chorus::drop(&b, x) > half).collect();
        let runs = cut.windows(2).filter(|p| !p[0] && p[1]).count() + usize::from(cut[0]);
        let wide = cut.iter().filter(|&&c| c).count() as f32 / runs as f32;
        (runs, wide)
    };
    let (comb, comb_w) = notches(ChorusMode::Flanger);
    let (dips, dips_w) = notches(ChorusMode::Phaser);
    assert!(comb >= 5, "{comb} notches");
    assert!((2..=3).contains(&dips), "{dips} dips");
    assert!(
        dips_w > 2.0 * comb_w,
        "dips {dips_w} px, notches {comb_w} px"
    );
}

#[test]
fn the_focused_part_lights_as_the_braids_do() {
    let set = [0.5, 0.6, 0.6];
    for m in [ChorusMode::Flanger, ChorusMode::Phaser] {
        let frame = 23;
        let b = |focus| at(m, set, focus, frame);
        let ink = |focus| count(&shot(b(focus)), whole(), theme::INK);
        let mid = |focus| count(&shot(b(focus)), whole(), theme::MID);
        // Nothing in INK unfocused; MID only from the floor line.
        assert_eq!(ink(None), 0, "{m:?}");
        // MODE: a dot at the left.
        let x0 = theme::BRAID_X + 2;
        let fb = shot(b(Some(BraidPart::Mode)));
        assert!(count(&fb, x0..x0 + 5, theme::INK) >= 9, "{m:?} MODE");
        assert_eq!(count(&fb, x0 + 5..x0 + 76, theme::INK), 0, "{m:?} MODE");
        // RATE: a bead riding the leading notch.
        let lx = x0 + chorus::lead(&b(None)).round() as i32;
        let fb = shot(b(Some(BraidPart::Rate)));
        assert!(count(&fb, lx - 2..lx + 3, theme::INK) >= 9, "{m:?} RATE");
        assert_eq!(
            count(&fb, whole(), theme::INK),
            count(&fb, lx - 2..lx + 3, theme::INK)
        );
        // DEPTH: the sweep's bounds dotted in MID.
        assert!(mid(Some(BraidPart::Depth)) >= mid(None) + 8, "{m:?} DEPTH");
        assert_eq!(ink(Some(BraidPart::Depth)), 0);
        // MIX: the floor line in INK, the whole width.
        assert!(ink(Some(BraidPart::Mix)) >= 70, "{m:?} MIX");
    }
}

#[test]
fn the_pictures_stay_in_the_box() {
    let (x, y, w, h) = (
        theme::BRAID_X,
        theme::BRAID_Y,
        theme::BRAID_W,
        theme::BRAID_H,
    );
    for m in [ChorusMode::Flanger, ChorusMode::Phaser] {
        for v in [0.0, 0.5, 1.0] {
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
