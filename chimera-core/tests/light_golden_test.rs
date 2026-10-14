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

const GOLDENS: &[(&str, u64)] = &[
    ("engine_algo", 0x7cf6cb36ae4d4e91),
    ("algo_formant_page", 0x6095fa77c712dbab),
    ("algo_op_page_3", 0x380578a757df3272),
    ("algo_op_page_3_turned", 0x4e60271ac64ddbc1),
    ("algo_op_page_frmt", 0xbd6f315cbe08dd1c),
    ("algo_op_page_nois", 0x250a6ac9df874997),
    ("flt_clean", 0xcc4a729ff4ba5faf),
    ("settings_top", 0xf1655533215f4e4b),
    ("settings_personal_theme", 0x3a2a9ae7815335b2),
    // Re-recorded: FACTORY joins the title, B ◂▸ moves to hint line 1.
    // Then the formant merge: the pool meter's cost.
    ("sound_browser", 0xf31114affd5e4164),
    ("toast_exfat", 0xd8ee278545272c31),
    // Re-recorded on the formant merge: the pool meter's cost.
    ("factory_described", 0xf156c95bd722c0f3),
    ("factory_empty", 0xe0ed61157e6d73c3),
    ("factory_no_card", 0x44e7d55b3b0d5a3f),
    ("factory_coming_soon", 0x6c02b7c62f5a5a32),
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
    ]
    .map(raw);
    for name in case_names() {
        for &p in &render(name).px {
            let neutral = (p >> 11) == (p & 31) && (p >> 6 & 31) == (p >> 11);
            assert!(neutral || tokens.contains(&p), "{name}: {p:04x}");
        }
    }
}
