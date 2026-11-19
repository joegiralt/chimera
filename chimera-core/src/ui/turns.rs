//! The frame's turns as the edit path takes them (ADR 0279): each
//! encoder's ticks, plain or fine, and what they did to SEQ's hold. Bank
//! 2's `ui::feel` answers them; closed, `FrameTurns::closed`.

use chimera_hal::{Accel, NUM_ENCODERS, Ticks, Turn};

use crate::ui::hold::SeqTurn;

/// One encoder this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncTurn {
    Still,
    /// The cell's curve.
    Plain(Ticks),
    /// SEQ held: a tick a detent.
    Fine(Ticks),
}

/// Every encoder this frame, and SEQ's hold after them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameTurns {
    pub enc: [EncTurn; NUM_ENCODERS],
    pub seq: SeqTurn,
}

impl FrameTurns {
    /// Bank 2 closed: a parameter's curve everywhere, no fine step, SEQ's
    /// hold as it was.
    pub fn closed(turns: [Turn; NUM_ENCODERS]) -> Self {
        FrameTurns {
            enc: turns.map(|t| match t.detents().get() {
                0 => EncTurn::Still,
                _ => EncTurn::Plain(Accel::Param.ticks(t)),
            }),
            seq: SeqTurn::Untouched,
        }
    }
}
