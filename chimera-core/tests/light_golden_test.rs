//! Light-mode screen goldens (ADR 0126): a handful of the dark goldens'
//! cases, drawn by the unchanged renderer and pushed through INVERT's
//! palette (TEAL, BLACK -2) as the display shell does. Dark goldens live in
//! `screen_golden_test`; these add to them and never replace one.
//!
//! `SCREEN_DUMP=<dir>` writes `<case>_light.ppm` beside the dark ones.

#![cfg(feature = "orbit")]

mod common;
mod screen;

use chimera_core::ui::theme_settings::{Accent, Black, ThemeSettings, palette};
use screen::*;

// Re-recorded for #614 (Task 7, layout A): the Algo map without DRV in
// engine_algo, algo_formant_page and the algo_op_page_* four; flt_clean's
// cells in layout A.// Re-recorded in PR 4 (Task 8): the PLACE cell's FX/FLT glyph.
const GOLDENS: &[(&str, u64)] = &[
    ("engine_algo", 0x1f9c0460b53edbe5),
    // Re-recorded: a value's unit sits small beside its number (ADR 0146).
    ("algo_formant_page", 0x667903c194e3942f),
    ("algo_op_page_3", 0x4fa1837d43fa861a),
    ("algo_op_page_3_turned", 0x0fbe9f33b8f64f79),
    ("algo_op_page_frmt", 0x3248e92fc9e0cf08),
    ("algo_op_page_nois", 0x41d2669aa19cb3bf),
    // Re-recorded on the merge of main (#614): the filter picture in layout A.
    ("flt_clean", 0xb23c27d9c36f48d1),
    ("settings_top", 0xf1655533215f4e4b),
    ("settings_personal_theme", 0x3a2a9ae7815335b2),
    // Re-recorded: FACTORY joins the title, B ◂▸ moves to hint line 1.
    // Then the formant merge: the pool meter's cost.
    ("sound_browser", 0x4ff3933ac46b57e1),
    ("toast_exfat", 0xd8ee278545272c31),
    // Re-recorded on the formant merge: the pool meter's cost.
    ("factory_described", 0x4ce8bf03c8bdd692),
    ("factory_empty", 0xde1e1dcfa7d715bf),
    ("factory_no_card", 0x2d920eb72d59c123),
    ("factory_tape_row", 0x6737af4a028fc872),
];

fn light(name: &str) -> Fb {
    let p = palette(&ThemeSettings {
        invert: true,
        accent: Accent::Teal,
        black: Black::new(-2),
        ..ThemeSettings::DEFAULT
    });
    let mut fb = render(name);
    for px in &mut fb.px {
        *px = p.map_raw(*px);
    }
    fb.dump(&format!("{name}_light"));
    fb
}

#[test]
fn light_screens_match_their_goldens() {
    let got: Vec<(&str, u64)> = GOLDENS.iter().map(|&(n, _)| (n, light(n).hash())).collect();
    common::golden::check(GOLDENS, &got);
}

/// Light mode puts ink on a light ground: most of every screen is the
/// ground, and the dark-mode ground (near black) is nowhere on it.
#[test]
fn light_screens_are_mostly_white() {
    for &(name, _) in GOLDENS {
        let fb = light(name);
        // The overlay is drawn on a blank (0) screen: its ground flips to 0xFFDF.
        let white = fb.px.iter().filter(|&&p| p >> 11 >= 30).count();
        let share = if name.starts_with("toast") {
            0
        } else {
            fb.px.len() / 2
        };
        assert!(white > share && white > 1000, "{name}: {white}");
    }
}

/// The audit behind ADR 0126: every colour on every screen is a theme
/// token or a warm grey, so light mode's bitwise flip and five exact
/// swaps cover it. A new hard-coded colour fails here.
#[test]
fn every_screen_colour_is_a_theme_token_or_a_grey() {
    use chimera_core::ui::theme as t;
    use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
    let raw = |c| RawU16::from(c).into_inner();
    let tokens = [
        t::BG,
        t::INK,
        t::INK2,
        t::MID,
        t::BAR_REST,
        t::FAINT,
        t::ACCENT,
        t::ACCENT_SOFT,
        t::PANEL,
        t::PILL_EDGE,
        t::WARN,
        t::ALERT,
        t::PART_LIVE,
    ]
    .map(raw);
    for name in case_names() {
        for &p in &render(name).px {
            let neutral = (p >> 11) == (p & 31) && (p >> 6 & 31) == (p >> 11);
            assert!(neutral || tokens.contains(&p), "{name}: {p:04x}");
        }
    }
}
