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

fn main() {
    let mut display = DesktopDisplay::new();
    let mut controls = DesktopControls::new();

    let text_style = MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE);
    let mut enc_accum = [0i32; 7];

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

        // Clear screen
        Rectangle::new(Point::zero(), Size::new(240, 320))
            .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
            .draw(&mut display)
            .unwrap();

        // Title
        Text::new("CHIMERA v0.1.0", Point::new(10, 16), text_style)
            .draw(&mut display)
            .unwrap();

        // Encoder values
        let enc_names = ["A", "B", "C", "D", "E", "F", "Main"];
        for (i, name) in enc_names.iter().enumerate() {
            let s = format!("{}: {}", name, enc_accum[i]);
            let x = 10 + (i % 4) as i32 * 58;
            let y = 40 + (i / 4) as i32 * 16;
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
            let y = 90 + (i / 6) as i32 * 20;
            Text::new(name, Point::new(x, y), style)
                .draw(&mut display)
                .unwrap();
        }

        // Key help
        let help_style = MonoTextStyle::new(&FONT_6X10, Rgb565::CSS_DARK_GRAY);
        Text::new("Keys: 1-6=chains  arrows=nav", Point::new(10, 150), help_style)
            .draw(&mut display)
            .unwrap();
        Text::new("Q/A W/S E/D R/F T/G Y/H=enc", Point::new(10, 166), help_style)
            .draw(&mut display)
            .unwrap();
        Text::new("Space=MIX  M=Menu  Z/X=note", Point::new(10, 182), help_style)
            .draw(&mut display)
            .unwrap();

        display.flush();
        std::thread::sleep(std::time::Duration::from_millis(33));
    }
}
