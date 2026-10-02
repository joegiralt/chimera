mod audio;
mod controls;
mod display;
#[cfg(feature = "midi")]
mod midi;
#[cfg(test)]
mod qa;
mod store;

use chimera_core::project::{LOAD_ACK_TIMEOUT_MS, LOAD_LINK};
use chimera_core::scope::scope_buffer;
use chimera_core::storage::{Card, SystemSettings, SystemSync};
use chimera_core::ui::UiState;
use chimera_core::ui::animation::Pacer;
use chimera_core::ui::busy::{ToastStep, draw_busy, draw_toast};
use chimera_core::ui::perf::PerfTracker;
use chimera_core::ui::settings::CardCx;
use chimera_hal::store::Store;
use chimera_hal::{ChimeraDisplay, MidiChannel, MidiNote, Ms, Velocity};
use controls::DesktopControls;
use display::DesktopDisplay;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use store::DirStore;

/// The card: `CHIMERA_CARD`, or `chimera-card` made on first run. A
/// `CHIMERA_CARD` that doesn't exist is an empty slot.
fn card_dir() -> PathBuf {
    std::env::var_os("CHIMERA_CARD").map_or_else(
        || {
            let dir = PathBuf::from("chimera-card");
            // A failure shows as no card.
            let _ = std::fs::create_dir_all(&dir);
            dir
        },
        PathBuf::from,
    )
}

/// SYSTEM and its theme, then step 2: the last project, or NEW and why.
fn boot<S: Store>(
    ui: &mut UiState,
    card: &mut Card,
    store: &mut S,
) -> (SystemSync, SystemSettings) {
    // No card or a card fault shows at the project's boot; a SYSTEM
    // file that can't be read still applies the defaults silently:
    // https://github.com/joegiralt/chimera/issues/197
    let (sync, settings, _) = SystemSync::boot(card, store);
    ui.set_theme(settings.theme);
    ui.boot_project(card, store, settings.last_project);
    (sync, settings)
}

fn main() {
    let mut display = DesktopDisplay::new();
    let mut controls = DesktopControls::new();
    let (scope_w, mut scope_r) = Box::leak(Box::new(scope_buffer())).split();
    let mut audio = audio::DesktopAudio::new(scope_w);

    let mut ui = UiState::new();

    // Boot: SYSTEM and the last project behind the splash, held 1 s from
    // first light as on the chip; then the splash again in the card's theme.
    let first_light = std::time::Instant::now();
    display.set_theme(&ui.theme());
    let _ = chimera_core::ui::splash::draw(&mut display);
    display.flush();
    let mut store = DirStore::new(card_dir());
    let mut card = Card::new();
    let (mut sync, mut settings) = boot(&mut ui, &mut card, &mut store);
    display.set_theme(&ui.theme());
    let _ = chimera_core::ui::splash::draw(&mut display);
    display.flush();
    std::thread::sleep(std::time::Duration::from_secs(1).saturating_sub(first_light.elapsed()));
    let mut perf = PerfTracker::new();
    // The held key and the channel it was sent on, so its note-off follows
    // it even if the selected Part changes while it is held.
    let mut current_note: Option<(MidiChannel, MidiNote)> = None;
    let mut octave: i8 = 0; // -2 to +2
    let mut frame_start = Instant::now();
    // The loop runs near 30 Hz; animation at UI_FPS.
    let (mut pacer, _) = Pacer::start(Ms(first_light.elapsed().as_millis() as u32));
    // The toast's clock, read after the card work, as the firmware's is.
    let mut toast_at = Instant::now();

    while display.is_open() {
        let now = Instant::now();
        let frame_us = now.duration_since(frame_start).as_micros() as u32;
        frame_start = now;

        let keys = display.get_keys();
        controls.update_events(&keys, Ms(first_light.elapsed().as_millis() as u32));

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
            current_note = note.map(|n| (ui.project().part(ui.active_part).mix.channel, n));
            if let Some((ch, n)) = current_note {
                audio.note_on(ch, n, Velocity::DEFAULT);
            }
        }

        // UI framework handles navigation + encoder -> param binding
        ui.handle_input(&controls);
        // Card work the keys asked for, under BUSY; a load publishes
        // once the audio acks, or the timeout passes.
        if ui.card_pending() {
            let (y0, y1) = draw_busy(&mut display);
            display.flush_region(y0, y1);
        }
        let cx = CardCx {
            card: &mut card,
            store: &mut store,
            sync: &mut sync,
            settings: &mut settings,
        };
        ui.card_work(cx, &LOAD_LINK, |swap, p| {
            let deadline = Instant::now() + Duration::from_millis(LOAD_ACK_TIMEOUT_MS.into());
            let _ = swap.settle(&LOAD_LINK, || Instant::now() < deadline);
            audio.update(p.perf(), LOAD_LINK.epoch());
        });
        // Leaving SETTINGS syncs SYSTEM; a toast says how it went.
        ui.sync_system(&mut sync, &mut card, &mut store, &mut settings);
        display.set_theme(&ui.theme());
        if let Some(t) = pacer.due(Ms(first_light.elapsed().as_millis() as u32)) {
            ui.update(t);
        }

        // Push every Part and the FX to the audio thread.
        audio.update(ui.project().perf(), LOAD_LINK.epoch());

        ui.render_with_scope(&mut display, &perf.stats, scope_r.read());
        let now = Instant::now();
        let toast_ms = now.duration_since(toast_at).as_millis() as u32;
        toast_at = now;
        if let ToastStep::Show(text) = ui.step_toast(toast_ms) {
            draw_toast(&mut display, text.as_str());
        }

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
