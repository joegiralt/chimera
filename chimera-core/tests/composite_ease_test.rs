//! #284, #365: a composite glyph eases the params it reads that the page
//! has no cell for, as it eases its own cells', and follows them: a CC on
//! the delay's SAT (CHAR's cell) moves the rings on the DELAY page over a
//! few frames, never in one jump, and the band redraws them.

#![cfg(feature = "tape")]

mod screen;

use chimera_core::ui::UiState;
use chimera_core::ui::animation::UiTick;
use chimera_core::ui::glyph::Gauge;
use chimera_core::ui::perf::PerfStats;
use chimera_hal::EncoderId;
use screen::{DLY, Fb, Input, feed, render_ui, scope_fixture, settle, stack_ui, to_fx};

fn sat(ui: &UiState) -> f32 {
    match ui.shown_gauge() {
        Some(Gauge::Rings(r)) => r.sat,
        g => panic!("the DELAY page's band draws the rings, not {g:?}"),
    }
}

/// One frame: the UI's pass, then its draw.
fn frame(ui: &mut UiState, fb: &mut Fb) {
    ui.update(UiTick::for_test());
    ui.render_dirty_with_scope(fb, &PerfStats::zero(), &scope_fixture());
}

#[test]
fn an_off_page_cc_eases_the_rings_and_redraws_them() {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut ui = stack_ui();
    ui.project_mut().edit_fx().delay.saturation = 0.0;
    to_fx(&mut ui, DLY);
    feed(&mut ui, Input::turn(EncoderId::C, 1)); // FDBK: the rings
    settle(&mut ui);
    let mut fb = Fb::new();
    for _ in 0..3 {
        frame(&mut ui, &mut fb);
    }
    assert_eq!(sat(&ui), 0.0);
    // A CC: the stored value jumps.
    ui.project_mut().edit_fx().delay.saturation = 1.0;
    frame(&mut ui, &mut fb);
    let first = sat(&ui);
    assert!(first > 0.0 && first < 1.0, "SAT jumped to {first}");
    for _ in 0..60 {
        frame(&mut ui, &mut fb);
    }
    assert_eq!(sat(&ui), 1.0, "SAT eases home");
    // The band as drawn is the band a fresh draw makes (#365).
    assert!(fb.px == render_ui(&ui).px, "the band is stale");
}
