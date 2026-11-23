// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SMART EQ's pages (ADR 0296): ROLE on MIXER › PRT › EQ, its band the
//! Part's four live cuts; MIXER › MST › LVL, SMART EQ and AMOUNT in D and
//! E, its band every Part's.

mod common;
mod screen;

use chimera_core::dsp::comp::CompParams;
use chimera_core::part_eq::smart::{
    Amount, Band, CutDb, CutView, Role, SMART_VIEW, SmartEq, Switch,
};
use chimera_core::project::PartId;
use chimera_core::ui::UiState;
use chimera_core::ui::block_registry as reg;
use chimera_core::ui::theme;
use chimera_hal::{ButtonId, EncoderId};
use embedded_graphics::pixelcolor::Rgb565;
use screen::{Fb, Input, feed};

/// MIX + B1, back to PRT, EDIT down twice: EQ.
fn eq_page() -> UiState {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut ui = screen::stack_ui();
    feed(&mut ui, Input::chord(ButtonId::Mix, ButtonId::B1));
    feed(&mut ui, Input::press(ButtonId::Minus));
    feed(&mut ui, Input::press(ButtonId::Edit));
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.page_def().id, reg::PART_EQ.id);
    ui.step_toast(60_000);
    ui
}

/// The band's pixels in `c` (the viz band, under the cells).
fn band_px(shot: &Fb, c: Rgb565) -> usize {
    let mut n = 0;
    for y in 124..184 {
        for x in 0..240 {
            if shot.at(x, y) == c {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn role_is_the_eq_pages_third_cell() {
    let mut ui = eq_page();
    let p = PartId::ALL[0];
    assert_eq!(ui.project().part(p).mix.role, Role::Mid);
    feed(&mut ui, Input::turn(EncoderId::C, 1));
    assert_eq!(ui.project().part(p).mix.role, Role::Back);
    feed(&mut ui, Input::turn(EncoderId::C, -2));
    assert_eq!(ui.project().part(p).mix.role, Role::Front);
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    assert!(screen::cell_reads(&shot, 2, "FRONT"));
}

#[test]
fn the_eq_pages_band_shows_the_parts_live_cuts() {
    let mut ui = eq_page();
    let p = PartId::ALL[0];
    SMART_VIEW.publish(p, CutView::NONE);
    screen::settle(&mut ui);
    let none = band_px(&screen::render_ui(&ui), theme::ACCENT);
    let mut cuts = [CutDb::ZERO; 4];
    cuts[Band::Low.index()] = CutDb::MAX;
    cuts[Band::LowMid.index()] = CutDb::new(2.1);
    cuts[Band::High.index()] = CutDb::new(9.6);
    SMART_VIEW.publish(p, CutView::of(&cuts));
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    shot.dump("smart_eq_part_eq");
    let low = band_px(&shot, theme::ACCENT);
    SMART_VIEW.publish(p, CutView::NONE);
    // LOW's thin bar beside the Part's meter, its whole 3 × 38 px.
    assert!(low >= none + 3 * 38, "{none} → {low}");
}

#[test]
fn mst_lvl_turns_smart_eq_and_amount() {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut ui = screen::stack_ui();
    let master = reg::MIXER_CHANNEL_CHAIN
        .blocks
        .iter()
        .position(|b| b.def.id == reg::MASTER.id)
        .unwrap();
    screen::to_fx(&mut ui, master);
    feed(&mut ui, Input::press(ButtonId::Edit));
    assert_eq!(ui.page_def().id, reg::MASTER_LEVEL.id);
    assert_eq!(ui.project().perf().fx.comp.smart, SmartEq::OFF);
    feed(&mut ui, Input::turn(EncoderId::D, 1));
    feed(&mut ui, Input::turn(EncoderId::E, 10));
    let s = ui.project().perf().fx.comp.smart;
    assert_eq!(s.switch(), Switch::On);
    assert!(s.amount().code() > Amount::DEFAULT.code());
    assert_eq!(
        CompParams::DEFAULT.smart.amount(),
        Amount::DEFAULT,
        "AMOUNT starts at half"
    );
    // Cell F: the reverb return's role, BACK until moved.
    assert_eq!(ui.project().perf().fx.reverb.role, Role::Back);
    feed(&mut ui, Input::turn(EncoderId::F, -1));
    assert_eq!(ui.project().perf().fx.reverb.role, Role::Mid);
    screen::settle(&mut ui);
    let shot = screen::render_ui(&ui);
    assert!(screen::cell_reads(&shot, 3, "ON"));
    assert!(screen::cell_reads(&shot, 5, "MID"));
    assert!(band_px(&shot, theme::FAINT) > 0, "the Parts' columns drawn");
}
