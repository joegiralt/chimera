use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId, NUM_BUTTONS, NUM_ENCODERS};
use minifb::Key;

/// Maps keyboard to PreenFM3 controls.
///
/// Buttons:
///   1-6        -> B1-B6 (chain heads)
///   M          -> Menu
///   Left/Right -> Minus/Plus
///   Up         -> Seq (up in chain)
///   Down       -> Edit (down in chain)
///   Space      -> Mix (shift, hold)
///
/// Encoders (keyboard approximation):
///   Q/A -> Encoder A (up/down)
///   W/S -> Encoder B
///   E/D -> Encoder C
///   R/F -> Encoder D
///   T/G -> Encoder E
///   Y/H -> Encoder F
///   U/J -> Main encoder
pub struct DesktopControls {
    encoder_deltas: [i8; NUM_ENCODERS],
    button_current: [bool; NUM_BUTTONS],
    button_previous: [bool; NUM_BUTTONS],
}

impl DesktopControls {
    pub fn new() -> Self {
        Self {
            encoder_deltas: [0; NUM_ENCODERS],
            button_current: [false; NUM_BUTTONS],
            button_previous: [false; NUM_BUTTONS],
        }
    }

    /// Call once per frame with current key state
    pub fn update(&mut self, keys: &[Key]) {
        self.button_previous = self.button_current;
        self.button_current = [false; NUM_BUTTONS];
        self.encoder_deltas = [0; NUM_ENCODERS];

        for key in keys {
            match key {
                // Buttons
                Key::Key1 => self.button_current[ButtonId::B1 as usize] = true,
                Key::Key2 => self.button_current[ButtonId::B2 as usize] = true,
                Key::Key3 => self.button_current[ButtonId::B3 as usize] = true,
                Key::Key4 => self.button_current[ButtonId::B4 as usize] = true,
                Key::Key5 => self.button_current[ButtonId::B5 as usize] = true,
                Key::Key6 => self.button_current[ButtonId::B6 as usize] = true,
                Key::M => self.button_current[ButtonId::Menu as usize] = true,
                Key::Left => self.button_current[ButtonId::Minus as usize] = true,
                Key::Right => self.button_current[ButtonId::Plus as usize] = true,
                Key::Space => self.button_current[ButtonId::Mix as usize] = true,
                Key::Up => self.button_current[ButtonId::Seq as usize] = true,
                Key::Down => self.button_current[ButtonId::Edit as usize] = true,

                // Encoders (up = +1, down = -1)
                Key::Q => self.encoder_deltas[EncoderId::A as usize] += 1,
                Key::A => self.encoder_deltas[EncoderId::A as usize] -= 1,
                Key::W => self.encoder_deltas[EncoderId::B as usize] += 1,
                Key::S => self.encoder_deltas[EncoderId::B as usize] -= 1,
                Key::E => self.encoder_deltas[EncoderId::C as usize] += 1,
                Key::D => self.encoder_deltas[EncoderId::C as usize] -= 1,
                Key::R => self.encoder_deltas[EncoderId::D as usize] += 1,
                Key::F => self.encoder_deltas[EncoderId::D as usize] -= 1,
                Key::T => self.encoder_deltas[EncoderId::E as usize] += 1,
                Key::G => self.encoder_deltas[EncoderId::E as usize] -= 1,
                Key::Y => self.encoder_deltas[EncoderId::F as usize] += 1,
                Key::H => self.encoder_deltas[EncoderId::F as usize] -= 1,
                Key::U => self.encoder_deltas[EncoderId::Main as usize] += 1,
                Key::J => self.encoder_deltas[EncoderId::Main as usize] -= 1,
                _ => {}
            }
        }
    }
}

impl Controls for DesktopControls {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.encoder_deltas[id as usize]
    }

    fn button_state(&self, id: ButtonId) -> ButtonState {
        let curr = self.button_current[id as usize];
        let prev = self.button_previous[id as usize];
        match (prev, curr) {
            (false, true) => ButtonState::Pressed,
            (true, true) => ButtonState::Held,
            (true, false) => ButtonState::Released,
            (false, false) => ButtonState::Up,
        }
    }
}
