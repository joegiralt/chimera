mod audio;
mod controls;
mod display;
#[cfg(feature = "midi")]
mod midi;

use chimera_core::scope::scope_buffer;
use chimera_core::ui::UiState;
use chimera_core::ui::perf::PerfTracker;
use chimera_hal::{ChimeraDisplay, MidiChannel, MidiNote, Velocity};
use controls::DesktopControls;
use display::DesktopDisplay;
use std::time::Instant;

fn main() {
    let mut display = DesktopDisplay::new();
    let mut controls = DesktopControls::new();
    let (scope_w, mut scope_r) = Box::leak(Box::new(scope_buffer())).split();
    let mut audio = audio::DesktopAudio::new(scope_w);

    let mut ui = UiState::new();
    let mut perf = PerfTracker::new();
    // The held key and the channel it was sent on, so its note-off follows
    // it even if the selected Part changes while it is held.
    let mut current_note: Option<(MidiChannel, MidiNote)> = None;
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

        // Solo a DAC pair: F1-F3; F4 hears all three.
        for (key, pair) in [
            (minifb::Key::F1, 1),
            (minifb::Key::F2, 2),
            (minifb::Key::F3, 3),
            (minifb::Key::F4, 0),
        ] {
            if keys.contains(&key) {
                audio.solo(pair);
            }
        }

        // Piano keys play the selected Part's channel.
        let note = piano_note(&keys)
            .and_then(|n| MidiNote::new((n as i8 + octave * 12).clamp(0, 127) as u8));
        if note != current_note.map(|(_, n)| n) {
            if let Some((ch, n)) = current_note {
                audio.note_off(ch, n);
            }
            current_note = note.map(|n| (ui.performance.parts[ui.active_part].mix.channel, n));
            if let Some((ch, n)) = current_note {
                audio.note_on(ch, n, Velocity::DEFAULT);
            }
        }

        // UI framework handles navigation + encoder -> param binding
        ui.handle_input(&controls);
        ui.update();

        // Push every Part and the FX to the audio thread.
        audio.update(&ui.performance);

        ui.render_with_scope(&mut display, &perf.stats, scope_r.read());

        perf.record(frame_us, 0);

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
