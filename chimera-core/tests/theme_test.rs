//! System › Theme: settings, palette, gamma tables and the page itself.

mod screen;

use chimera_core::addr::{BlockRef, Blocks};
use chimera_core::block::Block;
use chimera_core::ui::block_registry::SYS_THEME;
use chimera_core::ui::theme;
use chimera_core::ui::theme_settings::{
    Accent, Black, Bright, Gamma, Palette, ThemeSettings, midpoint,
};
use chimera_hal::{ButtonId, EncoderId};
use embedded_graphics::pixelcolor::raw::{RawData, RawU16};
use embedded_graphics::pixelcolor::{Rgb565, RgbColor};
use screen::*;

#[test]
fn bright_clamps_and_steps_by_five() {
    assert_eq!(Bright::new(0).percent(), 10);
    assert_eq!(Bright::new(200).percent(), 100);
    assert_eq!(Bright::new(77).percent(), 75);
    assert_eq!(Bright::new(78).percent(), 80);
    let mut t = ThemeSettings::DEFAULT;
    assert_eq!(t.bright.percent(), 75);
    t.nudge(ThemeSettings::BRIGHT, 1);
    assert_eq!(t.bright.percent(), 80);
    t.nudge(ThemeSettings::BRIGHT, 100);
    assert_eq!(t.bright.percent(), 100);
    t.nudge(ThemeSettings::BRIGHT, -100);
    assert_eq!(t.bright.percent(), 10);
}

#[test]
fn bright_duty_does_not_overflow() {
    assert_eq!(Bright::DEFAULT.duty(12_000), 9_000);
    assert_eq!(Bright::new(100).duty(u16::MAX), u16::MAX);
    assert_eq!(Bright::new(10).duty(12_000), 1_200);
}

#[test]
fn black_clamps_to_minus_two_plus_four() {
    assert_eq!(Black::new(-9).get(), -2);
    assert_eq!(Black::new(9).get(), 4);
    let mut t = ThemeSettings::DEFAULT;
    t.nudge(ThemeSettings::BLACK, -1);
    assert_eq!(t.black.get(), -1);
    t.nudge(ThemeSettings::BLACK, -10);
    assert_eq!(t.black.get(), -2);
    t.nudge(ThemeSettings::BLACK, 10);
    assert_eq!(t.black.get(), 4);
}

#[test]
fn choices_clamp_at_their_ends() {
    let mut t = ThemeSettings::DEFAULT;
    t.nudge(ThemeSettings::GAMMA, 10);
    assert_eq!(t.gamma, Gamma::Punch);
    t.nudge(ThemeSettings::GAMMA, -1);
    assert_eq!(t.gamma, Gamma::Soft);
    t.nudge(ThemeSettings::ACCENT, 10);
    assert_eq!(t.accent, Accent::Ice);
    t.nudge(ThemeSettings::ACCENT, -10);
    assert_eq!(t.accent, Accent::Teal);
}

#[test]
fn boot_defaults() {
    let t = ThemeSettings::default();
    assert_eq!(t.bright.percent(), 75);
    assert_eq!(t.gamma, Gamma::Panel);
    assert_eq!(t.accent, Accent::Teal);
    assert_eq!(t.black.get(), 0);
    for s in t.specs() {
        assert_eq!(t.get(s.id), s.default, "{}", s.label);
    }
}

#[test]
fn gamma_tables_are_the_right_bytes() {
    // ILI9341 datasheet V1.11 E0h/E1h reset defaults.
    assert_eq!(
        Gamma::Panel.tables().positive,
        [
            0x08, 0x0E, 0x12, 0x05, 0x03, 0x09, 0x47, 0x86, 0x2B, 0x0B, 0x04, 0x00, 0x00, 0x00,
            0x00
        ]
    );
    assert_eq!(
        Gamma::Panel.tables().negative,
        [
            0x08, 0x1A, 0x20, 0x07, 0x0E, 0x05, 0x3A, 0x8A, 0x40, 0x04, 0x18, 0x0F, 0x3F, 0x3F,
            0x0F
        ]
    );
    // Adafruit's, as commit 7293a02 sent them.
    assert_eq!(
        Gamma::Punch.tables().positive,
        [
            0x0F, 0x31, 0x2B, 0x0C, 0x0E, 0x08, 0x4E, 0xF1, 0x37, 0x07, 0x10, 0x03, 0x0E, 0x09,
            0x00
        ]
    );
    assert_eq!(
        Gamma::Punch.tables().negative,
        [
            0x00, 0x0E, 0x14, 0x03, 0x11, 0x07, 0x31, 0xC1, 0x48, 0x08, 0x0F, 0x0C, 0x31, 0x36,
            0x0F
        ]
    );
    assert_eq!(
        Gamma::Soft.tables().positive,
        [
            0x0B, 0x1F, 0x1E, 0x08, 0x08, 0x08, 0x4A, 0xB3, 0x31, 0x09, 0x0A, 0x01, 0x07, 0x04,
            0x00
        ]
    );
    assert_eq!(
        Gamma::Soft.tables().negative,
        [
            0x04, 0x14, 0x1A, 0x05, 0x0F, 0x06, 0x35, 0xA5, 0x44, 0x06, 0x13, 0x0D, 0x38, 0x3A,
            0x0F
        ]
    );
}

/// The 8th byte packs two 4-bit fields; they average apart, never carrying.
#[test]
fn soft_averages_fields_not_bytes() {
    let mut a = [0u8; 15];
    let mut b = [0u8; 15];
    a[7] = 0x0F;
    b[7] = 0xF0;
    a[0] = 3;
    b[0] = 8;
    let m = midpoint(&a, &b);
    assert_eq!(m[7], 0x77);
    assert_eq!(m[0], 5);
}

#[test]
fn teal_and_black_zero_are_the_canonical_palette() {
    assert_eq!(Accent::Teal.color(), theme::ACCENT);
    assert_eq!(Accent::Teal.soft(), theme::ACCENT_SOFT);
    assert_eq!(Black::DEFAULT.ground(), theme::BG);
    assert_eq!(ThemeSettings::DEFAULT.palette(), Palette::IDENTITY);
}

#[test]
fn accents_are_distinct_from_each_other_and_the_greys() {
    let others = [
        theme::BG,
        theme::INK,
        theme::INK2,
        theme::MID,
        theme::BAR_REST,
        theme::FAINT,
        theme::WARN,
        theme::ALERT,
    ];
    for (i, a) in Accent::ALL.iter().enumerate() {
        for b in &Accent::ALL[..i] {
            assert_ne!(a.color(), b.color(), "{a:?} {b:?}");
        }
        assert!(!others.contains(&a.color()), "{a:?}");
    }
}

#[test]
fn black_steps_are_distinct_and_below_faint() {
    let grounds: Vec<Rgb565> = (Black::MIN..=Black::MAX)
        .map(|k| Black::new(k).ground())
        .collect();
    for (i, g) in grounds.iter().enumerate() {
        assert!(!grounds[..i].contains(g), "{g:?}");
        assert!(g.g() < theme::FAINT.g(), "{g:?}");
    }
    assert_eq!(Black::new(-2).ground(), Rgb565::new(0, 0, 0));
}

fn mapped(fb: &Fb, p: &Palette) -> Vec<u16> {
    fb.px.iter().map(|&px| p.map_raw(px)).collect()
}

/// TEAL, BLACK 0 through the display's palette changes no pixel of any golden.
#[test]
fn default_theme_leaves_every_golden_bit_identical() {
    let p = ThemeSettings::DEFAULT.palette();
    for (name, _) in CASES {
        let fb = render(name);
        assert_eq!(mapped(&fb, &p), fb.px, "{name}");
    }
}

fn raw(c: Rgb565) -> u16 {
    RawU16::from(c).into_inner()
}

/// Every other accent recolours the accent and its soft fill, and nothing else.
#[test]
fn each_accent_recolours_only_accent_pixels() {
    let fb = render("bigviz_filter");
    let (a, s) = (raw(theme::ACCENT), raw(theme::ACCENT_SOFT));
    assert!(fb.px.contains(&a) && fb.px.contains(&s));
    for accent in &Accent::ALL[1..] {
        let t = ThemeSettings {
            accent: *accent,
            ..ThemeSettings::DEFAULT
        };
        let out = mapped(&fb, &t.palette());
        for (&before, &after) in fb.px.iter().zip(&out) {
            if before == a {
                assert_eq!(after, raw(accent.color()), "{accent:?}");
            } else if before == s {
                assert_eq!(after, raw(accent.soft()), "{accent:?}");
                assert_ne!(after, s, "{accent:?}");
            } else {
                assert_eq!(after, before, "{accent:?}");
            }
        }
    }
}

/// Every BLACK step moves the ground (and the soft fill riding it), nothing else.
#[test]
fn each_black_step_moves_only_the_ground() {
    let fb = render("bigviz_filter");
    let (bg, s) = (raw(theme::BG), raw(theme::ACCENT_SOFT));
    for k in [-2, -1, 1, 2, 3, 4] {
        let t = ThemeSettings {
            black: Black::new(k),
            ..ThemeSettings::DEFAULT
        };
        let out = mapped(&fb, &t.palette());
        for (&before, &after) in fb.px.iter().zip(&out) {
            if before == bg {
                assert_eq!(after, raw(Black::new(k).ground()), "{k}");
                assert_ne!(after, bg, "{k}");
            } else if before != s {
                assert_eq!(after, before, "{k}");
            }
        }
    }
}

/// MENU, PLUS ×2 reaches THEME; its four encoders edit the UI's settings.
#[test]
fn theme_page_is_reachable_and_edits_the_settings() {
    let mut ui = chimera_core::ui::UiState::new();
    feed(&mut ui, Input::press(ButtonId::Menu));
    feed(&mut ui, Input::press(ButtonId::Plus));
    feed(&mut ui, Input::press(ButtonId::Plus));
    assert_eq!(ui.nav.active_block_def().id, SYS_THEME.id);
    assert_eq!(ui.theme(), ThemeSettings::DEFAULT);

    feed(&mut ui, Input::turn(EncoderId::A, 1));
    feed(&mut ui, Input::turn(EncoderId::B, 2));
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    feed(&mut ui, Input::turn(EncoderId::D, -1));
    let t = ui.theme();
    assert_eq!(t.bright.percent(), 80);
    assert_eq!(t.gamma, Gamma::Punch);
    assert_eq!(t.accent, Accent::Amber);
    assert_eq!(t.black.get(), -1);
    // The Sound is untouched: THEME lives in the UI.
    assert!(
        ui.performance
            .edit(0)
            .part
            .sound
            .params
            .block(BlockRef::Theme)
            .is_none()
    );
    // Empty slots edit nothing.
    feed(&mut ui, Input::turn(EncoderId::E, 3));
    feed(&mut ui, Input::turn(EncoderId::F, 3));
    assert_eq!(ui.theme(), t);
}
