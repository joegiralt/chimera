//! The frame's turns as the edit path takes them (ADR 0279): each
//! encoder's ticks, plain or fine, and SEQ's hold after them. Bank 2's
//! `ui::feel` answers them; closed, `FrameTurns::closed`.

use chimera_hal::{ALL_ENCODERS, ButtonId, Controls, NUM_ENCODERS, Ticks};

use crate::ui::hold::{SeqHold, SeqKey, SeqTurn};

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
    pub seq: SeqHold,
}

impl FrameTurns {
    /// Bank 2 closed: each encoder's parameter ticks
    /// (`Controls::encoder_delta`), no fine step, SEQ's hold as it was.
    #[inline(never)]
    pub fn closed(c: &dyn Controls) -> Self {
        FrameTurns {
            enc: ALL_ENCODERS.map(|id| match c.encoder_delta(id) {
                0 => EncTurn::Still,
                t => EncTurn::Plain(Ticks::new(t)),
            }),
            seq: SeqHold::of(
                SeqKey::of(c.edges(ButtonId::Seq), c.edges(ButtonId::Mix)),
                SeqTurn::Untouched,
            ),
        }
    }
}
