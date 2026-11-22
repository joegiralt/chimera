//! #667: under MODE LOOPFM, WAVE scales each oscillator's loop (ADR 0162),
//! so WAVE's glyph draws that oscillator's looped wave at its own depth:
//! it follows WAVE and BLEND, and the SHAPE LOOPFM ignores leaves it.

#![cfg(feature = "tape")]

mod screen;

use chimera_core::dsp::va::VaMode;
use chimera_core::params::EngineType;
use chimera_core::project::PartId;
use chimera_core::ui::theme::{FOCUS_BOTTOM, HEADER_BOTTOM, SCREEN_W, WAVE_X};
use embedded_graphics::pixelcolor::IntoStorage;
use screen::{load_init, render_ui, settle, stack_ui};

/// VA's home page, WAVE 1 focused, OSC 1 at `wave` and `shape`, MODE
/// `mode` at BLEND `blend`, settled.
fn va_ui(mode: VaMode, blend: f32, wave: f32, shape: f32) -> chimera_core::ui::UiState {
    chimera_core::bank2::DOOR_CELL.open_host_if_linked();
    let mut ui = stack_ui();
    load_init(&mut ui, EngineType::Va);
    {
        let va = &mut ui.project_mut().edit_part(PartId::ALL[0]).sound.params.va;
        va.mode = mode;
        va.amount = blend;
        va.wave[0] = wave;
        va.shape[0] = shape;
    }
    settle(&mut ui);
    ui
}

/// WAVE 1's glyph box on VA's home page (`va_ui`).
fn glyph(mode: VaMode, blend: f32, wave: f32, shape: f32) -> Vec<u16> {
    let fb = render_ui(&va_ui(mode, blend, wave, shape));
    let mut px = vec![];
    for y in HEADER_BOTTOM..FOCUS_BOTTOM {
        for x in WAVE_X..SCREEN_W {
            px.push(fb.at(x, y).into_storage());
        }
    }
    px
}

#[test]
fn loopfm_draws_the_loop_wave_sets() {
    let m = VaMode::LoopFm;
    assert!(
        glyph(m, 0.8, 0.5, 0.0) == glyph(m, 0.8, 0.5, 1.0),
        "SHAPE moves WAVE's glyph under LOOPFM, which ignores it"
    );
    assert!(glyph(m, 0.8, 0.2, 0.0) != glyph(m, 0.8, 0.9, 0.0), "WAVE");
    assert!(glyph(m, 0.2, 0.5, 0.0) != glyph(m, 0.9, 0.5, 0.0), "BLEND");
}

#[test]
fn other_modes_draw_the_oscillators_shape() {
    let m = VaMode::Mix;
    assert!(glyph(m, 0.8, 0.5, 0.0) != glyph(m, 0.8, 0.5, 1.0));
}

/// The glyph's depth is the engine's: `target_b0(BLEND, WAVE)`.
#[test]
fn the_loop_glyphs_depth_is_target_b0() {
    use chimera_core::dsp::va::lbfm::target_b0;
    use chimera_core::ui::glyph::{Gauge, LoopB0};
    for (blend, wave) in [(0.2, 0.5), (0.8, 0.5), (0.8, 0.9), (1.0, 1.0)] {
        let ui = va_ui(VaMode::LoopFm, blend, wave, 0.3);
        assert_eq!(
            ui.shown_gauge(),
            Some(Gauge::VaLoop(LoopB0::new(target_b0(blend, wave)))),
            "BLEND {blend} WAVE {wave}"
        );
    }
}

/// MODE is a choice: its glyph switches on the frame MODE does, never
/// through the modes between on the way (review of #667).
#[test]
fn modes_glyph_switches_with_mode_not_through_the_ones_between() {
    use chimera_core::ui::animation::UiTick;
    use chimera_core::ui::glyph::Gauge;
    let mut ui = va_ui(VaMode::Mix, 0.8, 0.5, 0.3);
    ui.project_mut()
        .edit_part(PartId::ALL[0])
        .sound
        .params
        .va
        .mode = VaMode::LoopFm;
    ui.update(UiTick::for_test());
    assert!(
        matches!(ui.shown_gauge(), Some(Gauge::VaLoop(_))),
        "{:?}",
        ui.shown_gauge()
    );
}

/// BLEND's crossfader names its ends by MODE as stored: a MODE CC lands
/// on its own names, never on the modes between (round 3 of #667's review).
#[test]
fn blends_ends_follow_mode_as_stored() {
    use chimera_core::ui::animation::UiTick;
    use chimera_core::ui::glyph::Gauge;
    use chimera_hal::EncoderId;
    let mut ui = va_ui(VaMode::Mix, 0.8, 0.5, 0.3);
    // BLEND focused.
    screen::feed(&mut ui, screen::Input::turn(EncoderId::D, 1));
    screen::settle(&mut ui);
    ui.project_mut()
        .edit_part(PartId::ALL[0])
        .sound
        .params
        .va
        .mode = VaMode::LoopFm;
    ui.update(UiTick::for_test());
    match ui.shown_gauge() {
        Some(Gauge::Blend { mode, .. }) => assert_eq!(mode, VaMode::LoopFm),
        g => panic!("BLEND draws its crossfader, not {g:?}"),
    }
}
