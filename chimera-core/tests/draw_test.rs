//! Direction A primitives (`ui::draw`, `ui::theme`).

mod screen;

use chimera_core::ui::draw;
use chimera_core::ui::theme;
use embedded_graphics::pixelcolor::Rgb565;
use screen::Fb;

fn count(fb: &Fb, c: Rgb565) -> usize {
    (0..320).flat_map(|y| (0..240).map(move |x| (x, y))).filter(|&(x, y)| fb.at(x, y) == c).count()
}

#[test]
fn text_advance_matches_measured_width() {
    let mut fb = Fb::new();
    let adv = draw::text(&mut fb, &theme::FONT_VALUE, "CUTOFF", 10, 40, theme::INK);
    assert_eq!(adv, draw::text_width(&theme::FONT_VALUE, "CUTOFF", 0));
    assert_eq!(adv, 60, "helvB10 CUTOFF");
    assert!(count(&fb, theme::INK) > 0);
}

#[test]
fn tracking_adds_one_pixel_per_glyph() {
    let plain = draw::text_width(&theme::FONT_LABEL, "SHAPE", 0);
    assert_eq!(draw::text_width(&theme::FONT_LABEL, "SHAPE", 1), plain + 5);
    let mut fb = Fb::new();
    assert_eq!(draw::text_tracked(&mut fb, &theme::FONT_LABEL, "SHAPE", 0, 20, theme::MID, 1), plain + 5);
}

#[test]
fn unknown_glyphs_are_skipped_not_fatal() {
    let mut fb = Fb::new();
    draw::text(&mut fb, &theme::FONT_FOCUS, "→63", 10, 100, theme::INK);
    assert!(count(&fb, theme::INK) > 0, "the digits still draw");
}

#[test]
fn focus_font_is_large_and_label_font_small() {
    assert_eq!(theme::FONT_FOCUS.get_ascent(), 42);
    assert!(theme::FONT_LABEL.get_ascent() <= 8);
}

#[test]
fn unipolar_bar_fills_from_the_left() {
    let mut fb = Fb::new();
    draw::bar(&mut fb, 10, 10, 62, 2, 0.5, false, theme::FAINT, theme::ACCENT);
    let lit: Vec<i32> = (0..240).filter(|&x| fb.at(x, 10) == theme::ACCENT).collect();
    assert_eq!(lit.first(), Some(&10));
    assert_eq!(lit.len(), 31);
    assert_eq!(fb.at(71, 10), theme::FAINT, "track to the end");
}

#[test]
fn zero_bar_still_shows_two_pixels() {
    let mut fb = Fb::new();
    draw::bar(&mut fb, 10, 10, 62, 2, 0.0, false, theme::FAINT, theme::ACCENT);
    assert_eq!(count(&fb, theme::ACCENT), 4);
}

#[test]
fn bipolar_bar_grows_from_the_centre() {
    for (v, left, right) in [(0.75, 41, 56), (0.25, 26, 41)] {
        let mut fb = Fb::new();
        draw::bar(&mut fb, 10, 10, 62, 2, v, true, theme::FAINT, theme::ACCENT);
        let lit: Vec<i32> = (0..240).filter(|&x| fb.at(x, 10) == theme::ACCENT).collect();
        assert_eq!((lit[0], *lit.last().unwrap() + 1), (left, right), "value {v}");
    }
}

#[test]
fn arc_gauge_lights_the_start_for_low_values_and_the_top_for_bipolar_zero() {
    let (cx, cy, r) = (100, 100, 28);
    // Unipolar 0.1: lit near 7:30 (lower left), not at 4:30 (lower right).
    let mut fb = Fb::new();
    draw::arc_gauge(&mut fb, cx, cy, r, 5, 0.1, false, theme::FAINT, theme::ACCENT);
    assert_eq!(fb.at(cx - 20, cy + 20), theme::ACCENT, "start at lower left");
    assert_eq!(fb.at(cx + 20, cy + 20), theme::FAINT, "end at lower right is track");
    // Bipolar centre: only a cap at 12:00.
    let mut fb = Fb::new();
    draw::arc_gauge(&mut fb, cx, cy, r, 5, 0.5, true, theme::FAINT, theme::ACCENT);
    assert_eq!(fb.at(cx, cy - r), theme::ACCENT, "12 o'clock");
    assert_eq!(fb.at(cx - 20, cy + 20), theme::FAINT);
}

#[test]
fn pill_stays_inside_its_box() {
    let mut fb = Fb::new();
    draw::pill(&mut fb, 50, 276, 34, 20, theme::ACCENT);
    for y in 0..320 {
        for x in 0..240 {
            let inside = (50..84).contains(&x) && (276..296).contains(&y);
            if !inside {
                assert_ne!(fb.at(x, y), theme::ACCENT, "({x},{y})");
            }
        }
    }
    assert_eq!(fb.at(67, 286), theme::ACCENT);
    assert_ne!(fb.at(50, 276), theme::ACCENT, "rounded corner");
}

#[test]
fn primitives_clip_without_panicking() {
    let mut fb = Fb::new();
    draw::dot(&mut fb, 239, 319, 4, theme::INK);
    draw::text(&mut fb, &theme::FONT_FOCUS, "127", 220, 330, theme::INK);
    assert!(fb.oob > 0, "the harness sees off-screen writes");
}

/// The palette is the mockup's (ADR 0016) and the accent is unique.
#[test]
fn palette_tokens() {
    assert_eq!(theme::ACCENT, Rgb565::new(15, 53, 25)); // #7fd4c8
    assert_eq!(theme::BG, Rgb565::new(1, 2, 1)); // #0a0b0d
    for c in [theme::BG, theme::INK, theme::INK2, theme::MID, theme::BAR_REST, theme::FAINT, theme::ACCENT_SOFT] {
        assert_ne!(c, theme::ACCENT);
    }
}
