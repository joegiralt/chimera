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

/// WAVE 1's glyph box on VA's home page, OSC 1 at `wave` and `shape`,
/// MODE `mode` at BLEND `blend`.
fn glyph(mode: VaMode, blend: f32, wave: f32, shape: f32) -> Vec<u16> {
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
    let fb = render_ui(&ui);
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
