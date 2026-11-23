// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The header's Part dots (#562): the current Part filled bright green, a
//! sounding one a ring, the rest small and faint.
#![cfg(feature = "orbit")]

mod common;
mod screen;

use chimera_core::project::{PartId, PartSet};
use chimera_core::ui::theme;
use chimera_hal::PART_BUTTONS;
use screen::*;

fn dot_x(k: usize) -> i32 {
    theme::PART_DOT_X + k as i32 * theme::PART_DOT_PITCH
}

/// Which dots are filled bright green.
fn lit(fb: &Fb) -> Vec<usize> {
    (0..6)
        .filter(|&k| fb.at(dot_x(k), theme::PART_DOT_Y) == theme::PART_LIVE)
        .collect()
}

#[test]
fn the_lit_dot_follows_the_part_buttons() {
    for name in ["engine_algo", "mixer_sends"] {
        let mut ui = ui_for(name);
        for (k, b) in PART_BUTTONS.into_iter().enumerate() {
            tap(&mut ui, b);
            settle(&mut ui);
            assert_eq!(lit(&render_ui(&ui)), [k], "{name}: Part {}", k + 1);
        }
    }
}

#[test]
fn a_sounding_part_is_a_ring_and_a_quiet_one_faint() {
    let mut ui = ui_for("engine_algo");
    let live = PartSet::EMPTY.with(PartId::ALL[3]);
    ui.show_sounding(live);
    let fb = render_ui(&ui);
    let y = theme::PART_DOT_Y;
    assert_eq!(fb.at(dot_x(3), y), theme::BG, "a ring is hollow");
    assert_eq!(fb.at(dot_x(3) + theme::PART_DOT_R, y), theme::ACCENT);
    assert_eq!(fb.at(dot_x(4), y), theme::FAINT);
    assert_eq!(lit(&fb), [0]);
    ui.show_sounding(PartSet::EMPTY);
    assert_eq!(render_ui(&ui).at(dot_x(3), y), theme::FAINT);
}

/// The header region is keyed on the dots: a new mask or Part is dirty.
#[test]
fn the_header_region_keys_on_the_dots() {
    use chimera_core::ui::components::{HeaderStatus, PartDots};
    use chimera_core::ui::region::RegionData;
    let at = |current, live| {
        let parts = Some(PartDots { current, live });
        RegionData::header(
            0,
            0,
            0,
            0,
            0,
            HeaderStatus {
                parts,
                ..Default::default()
            },
        )
    };
    let (p0, p1) = (PartId::ALL[0], PartId::ALL[1]);
    let base = at(p0, PartSet::EMPTY);
    assert_eq!(base, at(p0, PartSet::EMPTY));
    assert_ne!(base, at(p1, PartSet::EMPTY));
    assert_ne!(base, at(p0, PartSet::EMPTY.with(p1)));
}

/// `SCREEN_DUMP=<dir>`: a look at a sound page, the mixer and
/// ORBIT with Part 1 current and Parts 3 and 5 sounding.
#[test]
fn dump_the_headers_with_dots() {
    let live = PartSet::EMPTY.with(PartId::ALL[2]).with(PartId::ALL[4]);
    for (case, name) in [
        ("engine_algo", "part_dots_sound"),
        ("mixer_sends", "part_dots_mixer"),
        ("orbit_rhythm", "part_dots_orbit"),
    ] {
        let mut ui = ui_for(case);
        ui.show_sounding(live);
        render_ui(&ui).dump(name);
    }
}
