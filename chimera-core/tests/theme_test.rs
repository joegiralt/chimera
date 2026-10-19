//! System › Theme: settings, palette, gamma tables and the page itself.

mod screen;

use chimera_core::addr::{BlockRead, BlockRef};
use chimera_core::block::Block;
use chimera_core::project::PartId;
use chimera_core::ui::block_registry::SYS_THEME;
use chimera_core::ui::theme;
use chimera_core::ui::theme_settings::{
    Accent, Black, Bright, Gamma, Pages, Palette, Saver, ThemeSettings, accent_color, accent_soft,
    duty, ground, midpoint, palette, tables,
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
    assert_eq!(t.bright.percent(), 70);
    t.nudge(ThemeSettings::BRIGHT, 1);
    assert_eq!(t.bright.percent(), 75);
    t.nudge(ThemeSettings::BRIGHT, 100);
    assert_eq!(t.bright.percent(), 100);
    t.nudge(ThemeSettings::BRIGHT, -100);
    assert_eq!(t.bright.percent(), 10);
}

#[test]
fn bright_duty_does_not_overflow() {
    assert_eq!(duty(Bright::DEFAULT, 12_000), 8_400);
    assert_eq!(duty(Bright::new(100), u16::MAX), u16::MAX);
    assert_eq!(duty(Bright::new(10), 12_000), 1_200);
}

#[test]
fn black_clamps_to_minus_two_plus_four() {
    assert_eq!(Black::new(-9).get(), -2);
    assert_eq!(Black::new(9).get(), 4);
    let mut t = ThemeSettings::DEFAULT;
    assert_eq!(t.black.get(), -2);
    t.nudge(ThemeSettings::BLACK, 1);
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
    assert_eq!(t.bright.percent(), 70);
    assert_eq!(t.gamma, Gamma::Punch);
    assert_eq!(t.accent, Accent::Teal);
    assert_eq!(t.black.get(), -2);
    for s in t.specs() {
        assert_eq!(t.get(s.id), s.default, "{}", s.label);
    }
}

#[test]
fn gamma_tables_are_the_right_bytes() {
    // ILI9341 datasheet V1.11 E0h/E1h reset defaults.
    assert_eq!(
        tables(Gamma::Panel).positive,
        [
            0x08, 0x0E, 0x12, 0x05, 0x03, 0x09, 0x47, 0x86, 0x2B, 0x0B, 0x04, 0x00, 0x00, 0x00,
            0x00
        ]
    );
    assert_eq!(
        tables(Gamma::Panel).negative,
        [
            0x08, 0x1A, 0x20, 0x07, 0x0E, 0x05, 0x3A, 0x8A, 0x40, 0x04, 0x18, 0x0F, 0x3F, 0x3F,
            0x0F
        ]
    );
    // Adafruit's, as commit 7293a02 sent them.
    assert_eq!(
        tables(Gamma::Punch).positive,
        [
            0x0F, 0x31, 0x2B, 0x0C, 0x0E, 0x08, 0x4E, 0xF1, 0x37, 0x07, 0x10, 0x03, 0x0E, 0x09,
            0x00
        ]
    );
    assert_eq!(
        tables(Gamma::Punch).negative,
        [
            0x00, 0x0E, 0x14, 0x03, 0x11, 0x07, 0x31, 0xC1, 0x48, 0x08, 0x0F, 0x0C, 0x31, 0x36,
            0x0F
        ]
    );
    assert_eq!(
        tables(Gamma::Soft).positive,
        [
            0x0B, 0x1F, 0x1E, 0x08, 0x08, 0x08, 0x4A, 0xB3, 0x31, 0x09, 0x0A, 0x01, 0x07, 0x04,
            0x00
        ]
    );
    assert_eq!(
        tables(Gamma::Soft).negative,
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
    assert_eq!(accent_color(Accent::Teal), theme::ACCENT);
    assert_eq!(accent_soft(Accent::Teal), theme::ACCENT_SOFT);
    assert_eq!(ground(Black::ZERO), theme::BG);
    assert_eq!(palette(&ThemeSettings::NEUTRAL), Palette::IDENTITY);
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
        theme::PART_LIVE,
    ];
    for (i, a) in Accent::ALL.iter().enumerate() {
        for b in &Accent::ALL[..i] {
            assert_ne!(accent_color(*a), accent_color(*b), "{a:?} {b:?}");
        }
        assert!(!others.contains(&accent_color(*a)), "{a:?}");
    }
}

#[test]
fn black_steps_are_distinct_and_below_faint() {
    let grounds: Vec<Rgb565> = (Black::MIN..=Black::MAX)
        .map(|k| ground(Black::new(k)))
        .collect();
    for (i, g) in grounds.iter().enumerate() {
        assert!(!grounds[..i].contains(g), "{g:?}");
        assert!(g.g() < theme::FAINT.g(), "{g:?}");
    }
    assert_eq!(ground(Black::new(-2)), Rgb565::new(0, 0, 0));
}

fn mapped(fb: &Fb, p: &Palette) -> Vec<u16> {
    fb.px.iter().map(|&px| p.map_raw(px)).collect()
}

/// TEAL, BLACK 0 through the display's palette changes no pixel of any golden.
/// The goldens are recorded with ORBIT built.
#[cfg(feature = "orbit")]
#[test]
fn default_theme_leaves_every_golden_bit_identical() {
    let p = palette(&ThemeSettings::NEUTRAL);
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
            ..ThemeSettings::NEUTRAL
        };
        let out = mapped(&fb, &palette(&t));
        for (&before, &after) in fb.px.iter().zip(&out) {
            if before == a {
                assert_eq!(after, raw(accent_color(*accent)), "{accent:?}");
            } else if before == s {
                assert_eq!(after, raw(accent_soft(*accent)), "{accent:?}");
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
        let out = mapped(&fb, &palette(&t));
        for (&before, &after) in fb.px.iter().zip(&out) {
            if before == bg {
                assert_eq!(after, raw(ground(Black::new(k))), "{k}");
                assert_ne!(after, bg, "{k}");
            } else if before != s {
                assert_eq!(after, before, "{k}");
            }
        }
    }
}

/// SETTINGS › PERSONALIZE › THEME; its four encoders edit the UI's settings.
#[test]
fn theme_page_is_reachable_and_edits_the_settings() {
    let mut ui = chimera_core::ui::UiState::new();
    to_leaf(&mut ui, &["PERSONALIZE", "THEME"]);
    assert_eq!(ui.page_def().id, SYS_THEME.id);
    assert_eq!(ui.theme(), ThemeSettings::DEFAULT);

    feed(&mut ui, Input::turn(EncoderId::A, 1));
    feed(&mut ui, Input::turn(EncoderId::B, 2));
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    feed(&mut ui, Input::turn(EncoderId::D, -1));
    let t = ui.theme();
    assert_eq!(t.bright.percent(), 75);
    assert_eq!(t.gamma, Gamma::Punch);
    assert_eq!(t.accent, Accent::Amber);
    assert_eq!(t.black.get(), -2);
    // The Sound is untouched: THEME lives in the UI.
    assert!(
        ui.project_mut()
            .edit_part(PartId::ALL[0])
            .sound
            .params
            .block(BlockRef::Theme)
            .is_none()
    );
    // F is INVERT: OFF stays OFF below zero, and ON sticks at the top.
    feed(&mut ui, Input::turn(EncoderId::F, -3));
    assert_eq!(ui.theme(), t);
    feed(&mut ui, Input::turn(EncoderId::F, 3));
    assert!(ui.theme().invert);
}

/// SYSTEM's hooks: `in_settings` follows MENU and the Part buttons, and
/// `set_theme` is what `theme` then reports.
#[test]
fn in_settings_and_set_theme() {
    let mut ui = chimera_core::ui::UiState::new();
    assert!(!ui.in_settings());
    tap(&mut ui, ButtonId::Menu);
    assert!(ui.in_settings());
    feed(&mut ui, Input::press(ButtonId::B1));
    assert!(!ui.in_settings());

    let t = ThemeSettings {
        bright: Bright::new(40),
        gamma: Gamma::Soft,
        accent: Accent::Lime,
        black: Black::new(1),
        hints: false,
        invert: false,
        saver: Saver::from_index(5),
        pages: Pages::Stack,
        text: chimera_core::ui::theme::TextSize::Large,
    };
    ui.set_theme(t);
    assert_eq!(ui.theme(), t);
}

/// A press after leaving SETTINGS takes the toast down before its time.
#[test]
fn input_dismisses_the_toast() {
    use chimera_core::project::Line;
    use chimera_core::storage::{Card, SystemSync};
    use chimera_core::ui::busy::ToastStep;
    let mut s = chimera_hal::testkit::MemStore::new(1);
    let mut card = Card::new();
    let (mut sync, mut cur, _) = SystemSync::boot(&mut card, &mut s);
    let mut ui = chimera_core::ui::UiState::new();
    tap(&mut ui, ButtonId::Menu);
    ui.sync_system(&mut sync, &mut card, &mut s, &mut cur);
    feed(&mut ui, Input::press(ButtonId::B1));
    ui.sync_system(&mut sync, &mut card, &mut s, &mut cur);
    assert_eq!(ui.step_toast(16), ToastStep::Show(Line::new("SAVED")));
    feed(&mut ui, Input::press(ButtonId::B2));
    assert_eq!(ui.step_toast(16), ToastStep::Ended);
    assert_eq!(ui.step_toast(16), ToastStep::Idle);
}

// --- INVERT (ADR 0126) ---

use chimera_core::ui::theme_settings::{
    ALERT_LIGHT, LIVE_LIGHT, WARN_LIGHT, accent_light, ground_light, splash_palette,
};

fn light(black: i8, accent: Accent) -> ThemeSettings {
    ThemeSettings {
        invert: true,
        black: Black::new(black),
        accent,
        ..ThemeSettings::DEFAULT
    }
}

/// WCAG 2 contrast ratio, true sRGB.
fn contrast(a: Rgb565, b: Rgb565) -> f64 {
    let lum = |c: Rgb565| {
        let lin = |v: u32| {
            let v = v as f64 / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let (r, g, bl) = (c.r() as u32, c.g() as u32, c.b() as u32);
        0.2126 * lin(r << 3 | r >> 2)
            + 0.7152 * lin(g << 2 | g >> 4)
            + 0.0722 * lin(bl << 3 | bl >> 2)
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

#[test]
fn invert_is_off_by_default_and_toggles() {
    let mut t = ThemeSettings::DEFAULT;
    assert!(!t.invert);
    t.nudge(ThemeSettings::INVERT, 1);
    assert!(t.invert);
    assert_eq!(t.normalized(ThemeSettings::INVERT), 1.0);
}

#[test]
fn light_mode_flips_every_other_colour_and_twice_is_the_identity() {
    let p = palette(&light(0, Accent::Teal));
    let swapped = [
        theme::BG,
        theme::ACCENT,
        theme::ACCENT_SOFT,
        theme::WARN,
        theme::ALERT,
        theme::PART_LIVE,
    ]
    .map(raw);
    for px in 0..=u16::MAX {
        if swapped.contains(&px) || swapped.contains(&!px) {
            continue;
        }
        let f = p.map_raw(px);
        assert_eq!(p.map_raw(f), px, "{px:04x}");
    }
    let (g, f) = (Rgb565::new(10, 20, 10), p.map(Rgb565::new(10, 20, 10)));
    assert_eq!((f.r(), f.g(), f.b()), (21, 43, 21), "{g:?}");
}

#[test]
fn light_greys_keep_their_order_and_their_contrast() {
    let p = palette(&light(-2, Accent::Teal));
    let ladder = [
        theme::INK,
        theme::INK2,
        theme::MID,
        theme::BAR_REST,
        theme::FAINT,
    ]
    .map(|c| p.map(c));
    assert_eq!(p.bg, Rgb565::new(31, 63, 31), "BLACK -2 is white");
    let l = |c: Rgb565| contrast(c, Rgb565::new(0, 0, 0));
    for w in ladder.windows(2) {
        assert!(l(w[0]) < l(w[1]), "{w:?}");
    }
    assert!(l(ladder[4]) < l(p.bg));
    assert!(contrast(ladder[0], p.bg) >= 12.0, "INK on white");
    assert!(contrast(ladder[2], p.bg) >= 3.0, "MID on white");
    assert_eq!(p.map(theme::BG), p.bg);
}

#[test]
fn black_takes_the_edge_off_white_in_light_mode() {
    let grounds: Vec<Rgb565> = (Black::MIN..=Black::MAX)
        .map(|k| ground_light(Black::new(k)))
        .collect();
    for (i, g) in grounds.iter().enumerate() {
        assert!(!grounds[..i].contains(g), "{g:?}");
        assert!(g.g() > theme::FAINT.g(), "{g:?}");
        if i > 0 {
            assert!(g.g() < grounds[i - 1].g());
        }
    }
}

/// Every ACCENT keeps 3:1 against the ground at every BLACK, both modes.
#[test]
fn every_accent_holds_3_to_1_in_both_modes() {
    for a in Accent::ALL {
        for k in Black::MIN..=Black::MAX {
            for invert in [false, true] {
                let t = ThemeSettings {
                    invert,
                    ..light(k, a)
                };
                let p = palette(&t);
                assert!(contrast(p.accent, p.bg) >= 3.0, "{a:?} {k} {invert}");
                assert_ne!(p.accent_soft, p.bg, "{a:?} {k} {invert}");
                if invert {
                    assert!(contrast(WARN_LIGHT, p.bg) >= 3.0, "WARN {k}");
                    assert!(contrast(ALERT_LIGHT, p.bg) >= 3.0, "ALERT {k}");
                    assert!(contrast(LIVE_LIGHT, p.bg) >= 3.0, "LIVE {k}");
                }
            }
        }
    }
}

/// The light accent is the pick darkened, never lightened or re-hued.
#[test]
fn light_accent_only_darkens() {
    for a in Accent::ALL {
        let (c, l) = (accent_color(a), accent_light(a));
        assert!(l.r() <= c.r() && l.g() <= c.g() && l.b() <= c.b(), "{a:?}");
    }
}

/// The Yellow Sign ignores INVERT.
#[test]
fn the_splash_palette_is_dark_mode() {
    let t = light(-2, Accent::Teal);
    let splash = splash_palette(&t);
    assert_eq!(splash, palette(&ThemeSettings { invert: false, ..t }));
    let yellow = raw(Rgb565::new(241 >> 3, 229 >> 2, 121 >> 3));
    assert_eq!(splash.map_raw(yellow), yellow);
}

/// `LIGHT_FLOOR` (231) is the darkest light ground, BLACK +4: (28, 57, 28)
/// expands to #e7e7e7 in 8 bits.
#[test]
fn the_light_floor_is_the_darkest_light_ground() {
    let floor = ground_light(Black::new(Black::MAX));
    assert_eq!(floor, Rgb565::new(28, 57, 28));
    let ch = |v: u8, bits: u32| (v as u32) << (8 - bits) | (v as u32) >> (2 * bits - 8);
    assert_eq!(
        (ch(floor.r(), 5), ch(floor.g(), 6), ch(floor.b(), 5)),
        (231, 231, 231)
    );
}

/// SETTINGS › PERSONALIZE › SCREENSAVER: its one cell is the idle time.
#[test]
fn the_screensaver_page_edits_its_idle_time() {
    use chimera_core::ui::block_registry::SYS_SAVER;
    let mut ui = chimera_core::ui::UiState::new();
    to_leaf(&mut ui, &["PERSONALIZE", "SCREENSAVER"]);
    assert_eq!(ui.page_def().id, SYS_SAVER.id);
    assert_eq!(ui.theme().saver, Saver::DEFAULT);
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_eq!(ui.theme().saver, Saver::from_index(8));
    feed(&mut ui, Input::turn(EncoderId::A, 9));
    assert_eq!(ui.theme().saver.ms(), Some(600_000), "held at 10 MIN");
    feed(&mut ui, Input::turn(EncoderId::A, -99));
    assert_eq!(ui.theme().saver, Saver::OFF);
}

/// The steps: 5 MIN by default, then the old ones keep their indices.
#[test]
fn the_saver_steps_keep_their_old_indices() {
    let ms: Vec<_> = (0..9).map(|i| Saver::from_index(i).ms()).collect();
    let s = |s: u32| Some(s * 1000);
    assert_eq!(
        ms,
        [
            None,
            s(5),
            s(7),
            s(10),
            s(30),
            s(60),
            s(120),
            s(300),
            s(600)
        ]
    );
    assert_eq!(Saver::DEFAULT.ms(), Some(300_000));
    assert_eq!(Saver::from_index(200), Saver::from_index(8));
}

/// OFF never starts the saver, however long the idle.
#[test]
fn saver_off_never_sleeps() {
    let mut ui = chimera_core::ui::UiState::new();
    to_leaf(&mut ui, &["PERSONALIZE", "SCREENSAVER"]);
    feed(&mut ui, Input::turn(EncoderId::A, -99));
    for _ in 0..1000 {
        ui.step_idle(60_000, false);
    }
    assert!(!ui.asleep());
}

/// Asleep, a button wakes and acts; a turn wakes and loses its first
/// detent: a turn you couldn't see never changes the sound (ADR 0147).
#[test]
fn a_button_wakes_and_acts_and_a_turn_loses_one_detent() {
    use chimera_core::ui::block_registry::SYS_SAVER;
    let mut ui = chimera_core::ui::UiState::new();
    to_leaf(&mut ui, &["PERSONALIZE", "SCREENSAVER"]);
    feed(&mut ui, Input::turn(EncoderId::A, -99));
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_eq!(ui.theme().saver, Saver::from_index(1));
    ui.step_idle(5_000, false);
    assert!(ui.asleep());
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert!(!ui.asleep());
    assert_eq!(
        ui.theme().saver,
        Saver::from_index(1),
        "one detent: only wakes"
    );
    ui.step_idle(5_000, false);
    assert!(ui.asleep());
    feed(&mut ui, Input::turn(EncoderId::A, 3));
    assert!(!ui.asleep());
    assert_eq!(ui.theme().saver, Saver::from_index(3), "3 detents: 2 act");
    ui.step_idle(10_000, false);
    assert!(ui.asleep());
    feed(&mut ui, Input::turn(EncoderId::A, -1));
    assert_eq!(ui.theme().saver, Saver::from_index(3), "backwards too");
    ui.step_idle(10_000, false);
    assert!(ui.asleep());
    tap(&mut ui, ButtonId::Menu);
    assert!(!ui.asleep());
    assert_ne!(ui.page_def().id, SYS_SAVER.id, "the button acted");
}

/// SETTINGS › PERSONALIZE › CHAIN MAP: its one cell is PAGES, ROW by
/// default, STACK one step up and held there (ADR 0145).
#[test]
fn the_chain_map_page_toggles_pages() {
    use chimera_core::ui::block_registry::SYS_MAP;
    let mut ui = chimera_core::ui::UiState::new();
    to_leaf(&mut ui, &["PERSONALIZE", "CHAIN MAP"]);
    assert_eq!(ui.page_def().id, SYS_MAP.id);
    assert_eq!(ui.theme().pages, Pages::Row);
    feed(&mut ui, Input::turn(EncoderId::A, 1));
    assert_eq!(ui.theme().pages, Pages::Stack);
    feed(&mut ui, Input::turn(EncoderId::A, 3));
    assert_eq!(ui.theme().pages, Pages::Stack);
    feed(&mut ui, Input::turn(EncoderId::A, -1));
    assert_eq!(ui.theme(), ThemeSettings::DEFAULT);
}
