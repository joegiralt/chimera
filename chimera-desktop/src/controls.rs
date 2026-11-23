// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

use chimera_core::console::Hand;
use chimera_hal::{
    ButtonId, ButtonState, Controls, Edges, EncoderId, Latch, Ms, NUM_BUTTONS, NUM_ENCODERS,
};
use minifb::Key;

/// Encoder A-F's keys, in `EncoderId` order.
pub(crate) const ENCODER_KEYS: [Key; NUM_ENCODERS] =
    [Key::Q, Key::W, Key::E, Key::R, Key::T, Key::Y];

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
/// both plays a note and turns an encoder. Pushes (SYSTEM's PUSH
/// ENCODERS, ADR 0140): Ctrl + Q..Y pushes that encoder, no turn (a tap
/// on release); Alt + Q..Y pushes and turns it, with Shift down.
pub struct DesktopControls {
    encoder_deltas: [i8; NUM_ENCODERS],
    latches: [Latch; NUM_BUTTONS],
    edges: [Edges; NUM_BUTTONS],
    push_latches: [Latch; NUM_ENCODERS],
    pushes: [Edges; NUM_ENCODERS],
    now_ms: Ms,
}

/// The button a key stands for.
pub(crate) fn button_for(key: Key) -> Option<ButtonId> {
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
            push_latches: [Latch::new(); NUM_ENCODERS],
            pushes: [Edges::default(); NUM_ENCODERS],
            now_ms: Ms(0),
        }
    }

    /// Call once per frame with the keys down now. A tap that starts and
    /// ends between two frames is not seen.
    #[cfg(test)]
    pub fn update_events(&mut self, down: &[Key], now_ms: Ms) {
        self.update_events_with(down, &Hand::default(), now_ms);
    }

    /// `update_events`, the console's hand (`key`, `turn`) pressing and
    /// turning with the keyboard.
    pub fn update_events_with(&mut self, down: &[Key], hand: &Hand, now_ms: Ms) {
        self.now_ms = now_ms;
        self.encoder_deltas = [0; NUM_ENCODERS];
        let any = |keys: [Key; 2]| down.iter().any(|k| keys.contains(k));
        let step = if any([Key::LeftShift, Key::RightShift]) {
            -1
        } else {
            1
        };
        let ctrl = any([Key::LeftCtrl, Key::RightCtrl]);
        let alt = any([Key::LeftAlt, Key::RightAlt]);
        for (i, key) in ENCODER_KEYS.iter().enumerate() {
            let held = down.contains(key);
            if held && !ctrl {
                self.encoder_deltas[i] = step;
            }
            self.encoder_deltas[i] = self.encoder_deltas[i].saturating_add(hand.detents[i]);
            let by_hand = hand.down & (1 << (NUM_BUTTONS + i)) != 0;
            self.push_latches[i].level(held && (ctrl || alt) || by_hand, now_ms);
            self.pushes[i] = self.push_latches[i].take();
        }

        let mut level: [bool; NUM_BUTTONS] = core::array::from_fn(|i| hand.down & (1 << i) != 0);
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

    fn now_ms(&self) -> Ms {
        self.now_ms
    }

    fn push(&self, id: EncoderId) -> Edges {
        self.pushes[id as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deltas(keys: &[Key]) -> [i8; NUM_ENCODERS] {
        let mut c = DesktopControls::new();
        c.update_events(keys, Ms(0));
        chimera_hal::ALL_ENCODERS.map(|e| c.encoder_delta(e))
    }

    /// The piano's black keys turn nothing (#93); Q-Y turn A-F, Shift down.
    #[test]
    fn piano_keys_turn_no_encoder() {
        assert_eq!(deltas(&[Key::S, Key::D, Key::G, Key::H]), [0; NUM_ENCODERS]);
        assert_eq!(deltas(&[Key::Q]), [1, 0, 0, 0, 0, 0]);
        assert_eq!(deltas(&[Key::LeftShift, Key::Y]), [0, 0, 0, 0, 0, -1]);
    }

    /// A held key ages: pressed once, then held with no new edge.
    #[test]
    fn a_held_key_is_pressed_then_held() {
        let mut c = DesktopControls::new();
        c.update_events(&[Key::M], Ms(10));
        assert_eq!(c.edges(ButtonId::Menu).pressed_at, Some(Ms(10)));
        c.update_events(&[Key::M], Ms(30));
        assert_eq!(c.button_state(ButtonId::Menu), ButtonState::Held);
        assert_eq!(c.now_ms(), Ms(30));
    }

    /// Ctrl + Q pushes A and turns nothing, a tap on release; Alt + Y
    /// pushes F and turns it, Shift down.
    #[test]
    fn ctrl_pushes_and_alt_pushes_and_turns() {
        let mut c = DesktopControls::new();
        c.update_events(&[Key::LeftCtrl, Key::Q], Ms(10));
        assert_eq!(c.push(EncoderId::A).pressed_at, Some(Ms(10)));
        assert_eq!(c.encoder_delta(EncoderId::A), 0);
        c.update_events(&[Key::LeftCtrl], Ms(30));
        assert_eq!(c.push(EncoderId::A).released_at, Some(Ms(30)));
        c.update_events(&[Key::LeftAlt, Key::LeftShift, Key::Y], Ms(50));
        assert!(c.push(EncoderId::F).down);
        assert_eq!(c.encoder_delta(EncoderId::F), -1);
        c.update_events(&[Key::Y], Ms(70));
        assert!(!c.push(EncoderId::F).down);
        assert_eq!(c.encoder_delta(EncoderId::F), 1);
    }
}
