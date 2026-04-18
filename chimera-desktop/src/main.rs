mod audio;
mod controls;
mod display;

use chimera_core::ui::perf::PerfTracker;
use chimera_core::ui::UiState;
use chimera_hal::ChimeraDisplay;
use controls::DesktopControls;
use display::DesktopDisplay;
use std::time::Instant;

/// Convert MIDI note number to frequency
fn note_to_freq(note: u8) -> f32 {
    440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0)
}

fn main() {
    let mut display = DesktopDisplay::new();
    let mut controls = DesktopControls::new();
    let audio = audio::DesktopAudio::new();

    let mut ui = UiState::new();
    let mut perf = PerfTracker::new();
    let mut current_note: Option<u8> = None;
    let mut frame_start = Instant::now();

    while display.is_open() {
        let now = Instant::now();
        let frame_us = now.duration_since(frame_start).as_micros() as u32;
        frame_start = now;

        let keys = display.get_keys();
        controls.update(&keys);

        // Piano keys -> audio
        let note = piano_note(&keys);
        if note != current_note {
            current_note = note;
            audio.set_frequency(note.map(note_to_freq).unwrap_or(0.0));
        }

        // UI framework handles navigation + encoder -> param binding
        ui.handle_input(&controls);
        ui.update();

        // Measure render time
        let render_start = Instant::now();
        ui.render(&mut display, &perf.stats);
        let render_us = render_start.elapsed().as_micros() as u32;

        perf.record(render_us, frame_us, 0);

        display.flush();
        std::thread::sleep(std::time::Duration::from_millis(33));
    }
}

/// Map held keyboard keys to MIDI note numbers (piano layout).
fn piano_note(keys: &[minifb::Key]) -> Option<u8> {
    let mappings: &[(minifb::Key, u8)] = &[
        (minifb::Key::Z, 60),
        (minifb::Key::S, 61),
        (minifb::Key::X, 62),
        (minifb::Key::D, 63),
        (minifb::Key::C, 64),
        (minifb::Key::V, 65),
        (minifb::Key::G, 66),
        (minifb::Key::B, 67),
        (minifb::Key::H, 68),
        (minifb::Key::N, 69),
    ];
    for &(key, note) in mappings {
        if keys.contains(&key) {
            return Some(note);
        }
    }
    None
}
