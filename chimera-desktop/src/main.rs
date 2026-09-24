mod audio;
mod controls;
mod display;

use chimera_core::ui::UiState;
use chimera_core::ui::perf::PerfTracker;
use chimera_hal::{ChimeraDisplay, MidiNote, Velocity};
use controls::DesktopControls;
use display::DesktopDisplay;
use std::time::Instant;

fn main() {
    let mut display = DesktopDisplay::new();
    let mut controls = DesktopControls::new();
    let mut audio = audio::DesktopAudio::new();

    let mut ui = UiState::new();
    let mut perf = PerfTracker::new();
    let mut current_note: Option<MidiNote> = None;
    let mut octave: i8 = 0; // -2 to +2
    let mut frame_start = Instant::now();

    while display.is_open() {
        let now = Instant::now();
        let frame_us = now.duration_since(frame_start).as_micros() as u32;
        frame_start = now;

        let keys = display.get_keys();
        controls.update(&keys);

        // Octave shift: [ and ]
        if keys.contains(&minifb::Key::LeftBracket) {
            octave = (octave - 1).max(-2);
        }
        if keys.contains(&minifb::Key::RightBracket) {
            octave = (octave + 1).min(2);
        }

        // Piano keys
        let note = piano_note(&keys)
            .and_then(|n| MidiNote::new((n as i8 + octave * 12).clamp(0, 127) as u8));
        if note != current_note {
            if let Some(n) = note {
                audio.note_on(n, Velocity::DEFAULT);
            } else {
                audio.note_off();
            }
            current_note = note;
        }

        // UI framework handles navigation + encoder -> param binding
        ui.handle_input(&controls);
        ui.update();

        // Push params + modulation routes to the audio thread (part 0)
        let sound = &ui.performance.parts[0].sound;
        audio.update(&sound.params, &sound.mod_state);

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
