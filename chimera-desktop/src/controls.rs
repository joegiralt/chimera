use chimera_hal::{
    ButtonId, ButtonState, Controls, Edges, EncoderId, Latch, NUM_BUTTONS, NUM_ENCODERS,
};
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
    latches: [Latch; NUM_BUTTONS],
    edges: [Edges; NUM_BUTTONS],
    now_ms: u32,
}

/// The button a key stands for.
fn button_for(key: Key) -> Option<ButtonId> {
    Some(match key {
        Key::Key1 => ButtonId::B1,
        Key::Key2 => ButtonId::B2,
        Key::Key3 => ButtonId::B3,
        Key::Key4 => ButtonId::B4,
        Key::Key5 => ButtonId::B5,
        Key::Key6 => ButtonId::B6,
        Key::M => ButtonId::Menu,
        Key::Left => ButtonId::Minus,
        Key::Right => ButtonId::Plus,
        Key::Space => ButtonId::Mix,
        Key::Up => ButtonId::Seq,
        Key::Down => ButtonId::Edit,
        _ => return None,
    })
}

impl DesktopControls {
    pub fn new() -> Self {
        Self {
            encoder_deltas: [0; NUM_ENCODERS],
            latches: [Latch::new(); NUM_BUTTONS],
            edges: [Edges::default(); NUM_BUTTONS],
            now_ms: 0,
        }
    }

    /// Call once per frame with the keys down now and those pressed and
    /// released since the last frame, so a tap between frames is kept.
    pub fn update_events(&mut self, down: &[Key], pressed: &[Key], released: &[Key], now_ms: u32) {
        self.now_ms = now_ms;
        self.encoder_deltas = [0; NUM_ENCODERS];
        let step = if down
            .iter()
            .any(|k| matches!(k, Key::LeftShift | Key::RightShift))
        {
            -1
        } else {
            1
        };
        for (i, key) in ENCODER_KEYS.iter().enumerate() {
            if down.contains(key) {
                self.encoder_deltas[i] = step;
            }
        }

        // Presses, then releases, then the level: a tap inside the frame
        // latches both edges and ends up.
        for (keys, level) in [(pressed, true), (released, false)] {
            for b in keys.iter().copied().filter_map(button_for) {
                self.latches[b as usize].level(level, now_ms);
            }
        }
        let mut level = [false; NUM_BUTTONS];
        for b in down.iter().copied().filter_map(button_for) {
            level[b as usize] = true;
        }
        for (i, latch) in self.latches.iter_mut().enumerate() {
            latch.level(level[i], now_ms);
            self.edges[i] = latch.take();
        }
    }
}

impl Controls for DesktopControls {
    fn encoder_delta(&self, id: EncoderId) -> i8 {
        self.encoder_deltas[id as usize]
    }

    fn button_state(&self, id: ButtonId) -> ButtonState {
        ButtonState::from_edges(self.edges[id as usize])
    }

    fn edges(&self, id: ButtonId) -> Edges {
        self.edges[id as usize]
    }

    fn now_ms(&self) -> u32 {
        self.now_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deltas(keys: &[Key]) -> [i8; NUM_ENCODERS] {
        let mut c = DesktopControls::new();
        c.update_events(keys, &[], &[], 0);
        chimera_hal::ALL_ENCODERS.map(|e| c.encoder_delta(e))
    }

    /// The piano's black keys turn nothing (#93); Q-Y turn A-F, Shift down.
    #[test]
    fn piano_keys_turn_no_encoder() {
        assert_eq!(deltas(&[Key::S, Key::D, Key::G, Key::H]), [0; NUM_ENCODERS]);
        assert_eq!(deltas(&[Key::Q]), [1, 0, 0, 0, 0, 0]);
        assert_eq!(deltas(&[Key::LeftShift, Key::Y]), [0, 0, 0, 0, 0, -1]);
    }

    /// A key down and up between two frames still reads as a press.
    #[test]
    fn a_tap_inside_one_frame_is_a_press() {
        let mut c = DesktopControls::new();
        c.update_events(&[], &[Key::Key3], &[Key::Key3], 100);
        assert_eq!(c.button_state(ButtonId::B3), ButtonState::Pressed);
        assert_eq!(c.edges(ButtonId::B3).released_at, Some(100));
        c.update_events(&[], &[], &[], 120);
        assert_eq!(c.button_state(ButtonId::B3), ButtonState::Up);
    }

    /// A held key ages: pressed once, then held with no new edge.
    #[test]
    fn a_held_key_is_pressed_then_held() {
        let mut c = DesktopControls::new();
        c.update_events(&[Key::M], &[Key::M], &[], 10);
        assert_eq!(c.edges(ButtonId::Menu).pressed_at, Some(10));
        c.update_events(&[Key::M], &[], &[], 30);
        assert_eq!(c.button_state(ButtonId::Menu), ButtonState::Held);
        assert_eq!(c.now_ms(), 30);
    }
}
