//! An encoder's push (ADR 0140), frame by frame: a turn while it is held
//! steps fine, and a push let go with no turn is a tap, which sets the
//! cell to its default. Pure: the edges and the turn in, the gesture out.

use chimera_hal::Edges;

/// One encoder's push across frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Push {
    held: bool,
    /// Turned since this push went down: its release is no tap.
    turned: bool,
}

/// What a frame's push did to its encoder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PushStep {
    /// This frame's turn is fine.
    pub fine: bool,
    /// A push let go that never turned: the cell goes to its default.
    pub tap: bool,
}

/// Keys and what they do, for the focus band.
pub const HINT: crate::ui::hint::Hint = &[
    crate::ui::hint::Gesture("PUSH", "DEFAULT"),
    crate::ui::hint::Gesture("PUSH + TURN", "FINE"),
];

impl Push {
    /// The frame's edges, and whether the encoder turned in it. A release
    /// and a new press in one frame end the old push first.
    pub fn step(&mut self, e: Edges, turned: bool) -> PushStep {
        let mut tap = false;
        let pressed = e.pressed_at.is_some();
        if pressed && e.released_at.is_some() && e.down {
            tap = self.held && !self.turned;
            self.held = false;
        }
        if pressed {
            *self = Push {
                held: true,
                turned: false,
            };
        }
        let fine = self.held && turned;
        self.turned |= fine;
        if self.held && !e.down {
            tap |= !self.turned;
            self.held = false;
        }
        PushStep { fine, tap }
    }
}
