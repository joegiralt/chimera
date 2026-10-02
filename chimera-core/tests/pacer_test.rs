//! `update` runs at UI_FPS however fast the loop spins: the delay rings
//! raced on an idle unit, whose loop runs far faster than 20 Hz.

use chimera_core::ui::UiState;
use chimera_core::ui::animation::{Pacer, UI_FPS};
use chimera_hal::Ms;

/// A loop spinning every ms for 1 s gets exactly UI_FPS frames.
#[test]
fn a_fast_loop_gets_ui_fps_frames_a_second() {
    let mut ui = UiState::new();
    let (mut pacer, _) = Pacer::start(Ms(0));
    let before = ui.clock().frame();
    let mut ticks = 0;
    for t in 1..=1000 {
        if let Some(tick) = pacer.due(Ms(t)) {
            ui.update(tick);
            ticks += 1;
        }
    }
    assert_eq!(ticks, UI_FPS);
    assert_eq!(ui.clock().frame().wrapping_sub(before), UI_FPS);
}

/// After a 500 ms stall, one frame a call and at most three in all: no
/// burst, no ten-frame catch-up.
#[test]
fn a_stall_catches_up_a_frame_or_two() {
    let (mut pacer, _) = Pacer::start(Ms(0));
    assert!(pacer.due(Ms(500)).is_some());
    let owed = (501..=510).filter(|&t| pacer.due(Ms(t)).is_some()).count();
    assert!(owed <= 2, "{owed}");
    // Then the steady rate again.
    let next = (511..=1510).filter(|&t| pacer.due(Ms(t)).is_some()).count();
    assert_eq!(next, UI_FPS as usize);
}

/// The ms clock wraps; the pacer doesn't notice.
#[test]
fn the_pacer_runs_across_a_wrap() {
    let start = u32::MAX - 20;
    let (mut pacer, _) = Pacer::start(Ms(start));
    let n = (1..=1000u32)
        .filter(|&t| pacer.due(Ms(start.wrapping_add(t))).is_some())
        .count();
    assert_eq!(n, UI_FPS as usize);
}
