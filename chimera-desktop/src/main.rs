mod audio;
mod controls;
mod display;

use chimera_hal::{ButtonState, ChimeraDisplay, Controls, EncoderId, ALL_BUTTONS};
use controls::DesktopControls;
use display::DesktopDisplay;
use embedded_graphics::mono_font::{ascii::FONT_6X10, MonoTextStyle};
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;

/// Convert MIDI note number to frequency
fn note_to_freq(note: u8) -> f32 {
    440.0 * libm::powf(2.0, (note as f32 - 69.0) / 12.0)
}

fn main() {
    let mut display = DesktopDisplay::new();
    let mut controls = DesktopControls::new();
    let audio = audio::DesktopAudio::new();

    let mut enc_accum = [0i32; 7];
    let mut current_note: Option<(&str, u8)> = None;

    while display.is_open() {
        let keys = display.get_keys();
        controls.update(&keys);

        // Accumulate encoder deltas
        for i in 0..7 {
            let id = match i {
                0 => EncoderId::A,
                1 => EncoderId::B,
                2 => EncoderId::C,
                3 => EncoderId::D,
                4 => EncoderId::E,
                5 => EncoderId::F,
                _ => EncoderId::Main,
            };
            enc_accum[i] += controls.encoder_delta(id) as i32;
        }

        // Piano keys -> MIDI notes
        current_note = if keys.contains(&minifb::Key::Z) {
            Some(("C4", 60))
        } else if keys.contains(&minifb::Key::S) {
            Some(("C#4", 61))
        } else if keys.contains(&minifb::Key::X) {
            Some(("D4", 62))
        } else if keys.contains(&minifb::Key::D) {
            Some(("D#4", 63))
        } else if keys.contains(&minifb::Key::C) {
            Some(("E4", 64))
        } else if keys.contains(&minifb::Key::V) {
            Some(("F4", 65))
        } else if keys.contains(&minifb::Key::G) {
            Some(("F#4", 66))
        } else if keys.contains(&minifb::Key::B) {
            Some(("G4", 67))
        } else if keys.contains(&minifb::Key::H) {
            Some(("G#4", 68))
        } else if keys.contains(&minifb::Key::N) {
            Some(("A4", 69))
        } else {
            None
        };

        if let Some((_, note)) = current_note {
            audio.set_frequency(note_to_freq(note));
        } else {
            audio.set_frequency(0.0);
        }

        // === RENDER ===

        // Clear screen
        Rectangle::new(Point::zero(), Size::new(240, 320))
            .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
            .draw(&mut display)
            .unwrap();

        let text_style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
        let dim_style = MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_DARK_GRAY);

        // Title
        Text::new("CHIMERA v0.1.0", Point::new(10, 16), text_style)
            .draw(&mut display)
            .unwrap();

        // Separator
        Rectangle::new(Point::new(0, 22), Size::new(240, 1))
            .into_styled(PrimitiveStyle::with_fill(Rgb565::CSS_DARK_GRAY))
            .draw(&mut display)
            .unwrap();

        // Encoder values
        let enc_names = ["A", "B", "C", "D", "E", "F", "Mn"];
        for (i, name) in enc_names.iter().enumerate() {
            let s = format!("{}:{:3}", name, enc_accum[i]);
            let x = 10 + (i % 4) as i32 * 58;
            let y = 40 + (i / 4) as i32 * 14;
            Text::new(&s, Point::new(x, y), text_style)
                .draw(&mut display)
                .unwrap();
        }

        // Button states
        let btn_names = [
            "B1", "B2", "B3", "B4", "B5", "B6", "MN", " -", " +", "MX", "ED", "SQ",
        ];
        for (i, name) in btn_names.iter().enumerate() {
            let id = ALL_BUTTONS[i];
            let state = controls.button_state(id);
            let color = match state {
                ButtonState::Pressed | ButtonState::Held => Rgb565::GREEN,
                _ => Rgb565::CSS_DARK_GRAY,
            };
            let style = MonoTextStyle::new(&FONT_6X10, color);
            let x = 10 + (i % 6) as i32 * 38;
            let y = 85 + (i / 6) as i32 * 18;
            Text::new(name, Point::new(x, y), style)
                .draw(&mut display)
                .unwrap();
        }

        // Current note display
        let (note_text, note_color) = if let Some((name, _)) = current_note {
            (format!("Note: {}", name), Rgb565::CSS_LIME_GREEN)
        } else {
            (String::from("Note: --"), Rgb565::CSS_DARK_GRAY)
        };
        let note_style = MonoTextStyle::new(&FONT_6X10, note_color);
        Text::new(&note_text, Point::new(10, 140), note_style)
            .draw(&mut display)
            .unwrap();

        // Dungeon map placeholder (bottom zone)
        Rectangle::new(Point::new(0, 213), Size::new(240, 1))
            .into_styled(PrimitiveStyle::with_fill(Rgb565::CSS_DARK_GRAY))
            .draw(&mut display)
            .unwrap();

        let chain_text = "[ENG]--[DRV]--[FLT]--[FLD]--[VCA]";
        Text::new(chain_text, Point::new(10, 240), dim_style)
            .draw(&mut display)
            .unwrap();
        Text::new("  ^", Point::new(10, 254), MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_LIME_GREEN))
            .draw(&mut display)
            .unwrap();

        // Help text
        Text::new("Z S X D C V G B H N = piano", Point::new(10, 290), dim_style)
            .draw(&mut display)
            .unwrap();
        Text::new("Q/A..Y/H = encoders  1-6=nav", Point::new(10, 304), dim_style)
            .draw(&mut display)
            .unwrap();

        display.flush();
        std::thread::sleep(std::time::Duration::from_millis(33));
    }
}
