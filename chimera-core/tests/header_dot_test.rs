// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The header's beat dot, alone in its binary (#1033): `UiState::update`
//! reads the Parts sounding from the process (`meter::sounding_parts`),
//! which any other test's audio block publishes, and the header's Part
//! dots follow it.

#![cfg(feature = "orbit")]

mod common;
mod screen;

use chimera_core::ui::UiState;
use chimera_core::ui::theme;

/// The beat dot, left of the sounding dot on every kind of page: absent
/// while ORBIT is stopped; running, filled on a beat's first tick, part
/// filled on the next, an outline by the third, and filled again on the
/// next beat. The CPU label moves left by one dot. The dirty path
/// redraws each step as a full render would.
#[test]
fn header_dot_pulses() {
    use chimera_core::orbit::runner::OrbitView;
    use chimera_core::ui::animation::UiTick;
    use chimera_core::ui::perf::PerfStats;
    use chimera_core::ui::region::RegionKind;
    use screen::{Fb, audio_fixture, orbit_view, scope_fixture};
    const LOAD: PerfStats = PerfStats {
        frame_us: 0,
        audio_us: 0,
        audio_load_pct: 42,
    };
    let full = |ui: &UiState| {
        let mut fb = Fb::new();
        ui.render_with_audio(&mut fb, &LOAD, Some(&audio_fixture()), &scope_fixture());
        fb
    };
    // The CPU label's right edge: the header's rightmost MID pixel.
    let cpu_right = |fb: &Fb| {
        (0..theme::SCREEN_W)
            .rev()
            .find(|&x| (0..theme::HEADER_BOTTOM).any(|y| fb.at(x, y) == theme::MID))
    };
    let (x, y) = (theme::ORBIT_DOT_X, theme::HEADER_DOT_Y);
    let r = theme::HEADER_DOT_R;
    for (name, cpu) in [
        ("algo_level", true),
        ("mixer_part", true),
        ("settings_top", false),
        ("settings_personal_theme", false),
        ("orbit_orb", true),
    ] {
        let mut ui = Box::new(screen::ui_for(name));
        ui.set_orbit_view(&OrbitView::default());
        screen::settle(&mut ui);
        let mut dirty = Fb::new();
        // The band the dots are in: a page's header, SETTINGS' crumbs.
        let head = |ui: &UiState| {
            ui.drawn_key(RegionKind::Header)
                .or(ui.drawn_key(RegionKind::Crumbs))
                .expect("a header band")
        };
        let mut step = |ui: &mut UiState, what: &str| {
            ui.update(UiTick::for_test());
            ui.render_dirty_with_audio(&mut dirty, &LOAD, Some(&audio_fixture()), &scope_fixture());
            let fb = full(ui);
            assert!(dirty.px == fb.px, "{name}: {what}, dirty");
            (fb, head(ui))
        };
        let (stopped, _) = step(&mut ui, "stopped");
        for dx in -r..=r {
            assert_eq!(stopped.at(x + dx, y), theme::BG, "{name}: stopped, no dot");
        }

        ui.set_orbit_view(&orbit_view(0.0));
        let (first, k1) = step(&mut ui, "the beat");
        assert_eq!(first.at(x, y), theme::ACCENT, "{name}: filled");
        assert_eq!(first.at(x + 2, y), theme::ACCENT, "{name}: filled");
        let (second, k2) = step(&mut ui, "fading");
        assert_eq!(second.at(x, y), theme::ACCENT, "{name}: part filled");
        assert_eq!(second.at(x + 2, y), theme::BG, "{name}: part filled");
        let (third, k3) = step(&mut ui, "faded");
        assert_eq!(third.at(x, y), theme::BG, "{name}: an outline");
        assert_eq!(third.at(x + r, y), theme::ACCENT, "{name}: an outline");
        let (still, k4) = step(&mut ui, "between beats");
        // The header's rows: below them MIXER › PRT's meters (ADR 0293)
        // read the shared Part peaks, which other tests' audio raises.
        let head_px = |fb: &Fb| fb.px[..screen::W * theme::HEADER_BOTTOM as usize].to_vec();
        assert!(
            head_px(&still) == head_px(&third),
            "{name}: still between beats"
        );
        // The band redraws as the dot fades, and not between beats.
        assert!(k1 != k2 && k2 != k3, "{name}: each fade step redraws");
        assert_eq!(k3, k4, "{name}: the band rests between beats");

        ui.set_orbit_view(&orbit_view(0.25));
        let (next, _) = step(&mut ui, "the next beat");
        assert_eq!(next.at(x + 2, y), theme::ACCENT, "{name}: the next beat");

        if cpu {
            let (s, run) = (cpu_right(&stopped), cpu_right(&third));
            assert_eq!(
                s.zip(run).map(|(s, run)| s - run),
                Some(theme::HEADER_DOT_X - theme::ORBIT_DOT_X),
                "{name}: the CPU label moves left by one dot"
            );
        }
    }
}
