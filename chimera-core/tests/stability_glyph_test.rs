// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! STABILITY's spirit level (#593): one glyph for VA's oscillator STAB and
//! the filter's (ADR 0168). The bubble rests between the marks at 1 and
//! leaves them by VA's depth law, `(1 − STAB)²`, to half the vial's travel at
//! 0, the ghost of its wander showing the rest. Still: drawn from whole
//! pixels of the set value, so nothing redraws at idle.
//!
//! `SCREEN_DUMP=<dir>` writes VA's STABILITY and the 2040's at a few values,
//! dark and light (`stab_*_dark`, `stab_*_light`).

mod common;
mod screen;

use chimera_core::block::ValFmt;
use chimera_core::dsp::filter::FilterKind;
use chimera_core::dsp::va::{VA_SPECS, VaParams};
use chimera_core::dsp::voice_fx::params::FxType;
use chimera_core::params::{EngineType, FILTER_SPECS, FilterParams};
use chimera_core::ui::UiState;
use chimera_core::ui::glyph::{BUBBLE_TRAVEL, FocusGlyph, Gauge, gauge};
use chimera_core::ui::theme;
use chimera_core::ui::theme_settings::{Accent, Black, ThemeSettings, palette};
use chimera_hal::EncoderId;
use screen::*;

fn at(v: f32) -> (i8, i8) {
    match gauge(FocusGlyph::Stability, v, ValFmt::Uni, |_| Gauge::None) {
        Gauge::Stability { offset, reach } => (offset, reach),
        g => panic!("{g:?}"),
    }
}

#[test]
fn both_stabilities_carry_the_level() {
    assert_eq!(
        VA_SPECS[usize::from(VaParams::STAB.0)].glyph,
        FocusGlyph::Stability
    );
    let f = FILTER_SPECS.iter().find(|s| s.id == FilterParams::STAB);
    assert_eq!(f.map(|s| s.glyph), Some(FocusGlyph::Stability));
}

/// At 1 the bubble sits between the marks with no wander; it leaves them
/// by `(1 − v)²`, never back, to half the travel at 0 with the wander
/// reaching the vial's ends.
#[test]
fn the_bubble_follows_the_depth_law() {
    assert_eq!(at(1.0), (0, 0));
    assert_eq!(at(0.0), ((BUBBLE_TRAVEL / 2) as i8, BUBBLE_TRAVEL as i8));
    assert_eq!(
        at(0.5),
        (3, 6),
        "a quarter of the travel at the Prophet setting"
    );
    let mut last = (0, 0);
    for t in (0..=127).rev() {
        let now = at(t as f32 / 127.0);
        assert!(now.0 >= last.0 && now.1 >= last.1, "{t}");
        last = now;
    }
    // Out of range lands on the ends.
    assert_eq!(at(2.0), at(1.0));
    assert_eq!(at(-1.0), at(0.0));
}

/// Still, keyed on its value alone: no frame term, no outside key.
#[test]
fn the_level_is_still() {
    let g = Gauge::Stability {
        offset: 3,
        reach: 6,
    };
    assert!(!g.animates());
    assert_eq!(g.outside_key(), 0);
    assert_eq!(chimera_core::ui::components::gauge_rect(&g), None);
}

fn shoot(ui: &UiState, name: &str) -> Fb {
    let fb = render_ui(ui);
    assert_eq!(fb.oob, 0, "{name}");
    for invert in [false, true] {
        let p = palette(&ThemeSettings {
            invert,
            accent: Accent::Teal,
            black: Black::new(-2),
            ..ThemeSettings::DEFAULT
        });
        let mut shot = Fb::new();
        shot.px = fb.px.iter().map(|&px| p.map_raw(px)).collect();
        shot.dump(&format!("{name}_{}", if invert { "light" } else { "dark" }));
    }
    fb
}

/// The vial's box on the band.
fn band(fb: &Fb) -> Vec<u16> {
    let x0 = theme::SCREEN_W - theme::MARGIN_X - theme::VIAL_W;
    (theme::ARC_CY - 16..theme::ARC_CY + 16)
        .flat_map(|y| (x0..x0 + theme::VIAL_W).map(move |x| fb.px[y as usize * W + x as usize]))
        .collect()
}

fn set(ui: &mut UiState, e: EncoderId, ticks: i8) {
    feed(ui, Input::turn(e, 127));
    feed(ui, Input::turn(e, -ticks));
    settle(ui);
}

/// VA's STABILITY (F) and the 2040's (FLT › FX F) draw the same level at
/// the same value, each value its own picture; at 127 the bubble covers
/// the vial's middle.
#[test]
fn both_pages_draw_the_level() {
    let mut va = UiState::new();
    load_init(&mut va, EngineType::Va);
    let mut flt = UiState::new();
    stack_map(&mut flt);
    on_flt_fx(&mut flt, FilterKind::K2040, FxType::Off);
    let mut seen = Vec::new();
    for down in [0i8, 32, 64, 127] {
        set(&mut va, EncoderId::F, down);
        set(&mut flt, EncoderId::F, down);
        assert_eq!(flt.params().filter.stability, va.params().va.stability);
        let a = band(&shoot(&va, &format!("stab_va_{:03}", 127 - down)));
        let b = band(&shoot(&flt, &format!("stab_2040_{:03}", 127 - down)));
        assert_eq!(a, b, "{down}");
        assert!(!seen.contains(&a), "{down}: a picture of its own");
        seen.push(a);
    }
    set(&mut va, EncoderId::F, 0);
    let fb = render_ui(&va);
    let mid = theme::SCREEN_W - theme::MARGIN_X - theme::VIAL_W / 2;
    assert_eq!(fb.at(mid, theme::ARC_CY), theme::ACCENT);
}
