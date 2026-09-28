use chimera_hal::{ButtonId, ButtonState, Controls, EncoderId, NUM_BUTTONS, NUM_ENCODERS};
use minifb::Key;

/// Encoder A-F's keys, in `EncoderId` order.
const ENCODER_KEYS: [Key; NUM_ENCODERS] = [Key::Q, Key::W, Key::E, Key::R, Key::T, Key::Y];

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
/// Encoders (keyboard approximation): Q W E R T Y turn A-F up, with Shift
/// down. The row below is the piano's black keys (`main.rs`), so no key
/// both plays a note and turns an encoder.
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
        let step = if keys
            .iter()
            .any(|k| matches!(k, Key::LeftShift | Key::RightShift))
        {
            -1
        } else {
            1
        };
        for (i, key) in ENCODER_KEYS.iter().enumerate() {
            if keys.contains(key) {
                self.encoder_deltas[i] = step;
            }
        }

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
        let i = id as usize;
        ButtonState::from_levels(self.button_previous[i], self.button_current[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deltas(keys: &[Key]) -> [i8; NUM_ENCODERS] {
        let mut c = DesktopControls::new();
        c.update(keys);
        chimera_hal::ALL_ENCODERS.map(|e| c.encoder_delta(e))
    }

    /// The piano's black keys turn nothing (#93); Q-Y turn A-F, Shift down.
    #[test]
    fn piano_keys_turn_no_encoder() {
        assert_eq!(deltas(&[Key::S, Key::D, Key::G, Key::H]), [0; NUM_ENCODERS]);
        assert_eq!(deltas(&[Key::Q]), [1, 0, 0, 0, 0, 0]);
        assert_eq!(deltas(&[Key::LeftShift, Key::Y]), [0, 0, 0, 0, 0, -1]);
    }
}
