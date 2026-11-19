//! Tap and hold from latched button edges: a tap acts on release, a hold
//! fires once at `HOLD_MS` while still down.

use chimera_hal::{ButtonId, Controls, Edges, Ms, PART_BUTTONS};

use crate::orbit::RINGS;

pub const HOLD_MS: u32 = 500;

#[derive(Clone, Copy, Debug)]
pub struct HoldGate {
    down_at: Option<Ms>,
    fired: bool,
    muted: bool,
}

impl Default for HoldGate {
    fn default() -> Self {
        Self::new()
    }
}

impl HoldGate {
    pub const fn new() -> Self {
        Self {
            down_at: None,
            fired: false,
            muted: false,
        }
    }

    /// One result per frame; in a stalled frame a hold of the new press wins over the old release.
    /// `muted`: the press under way yields nothing (MIX + MENU). Applied after
    /// a press in `e` resets the gate, before its release or hold is decided.
    pub fn step(&mut self, e: Edges, now: Ms, muted: bool) -> Option<Press> {
        let mut out = None;
        // Ms has no order: with both stamps, `down` says which came last.
        if let (true, Some(r)) = (e.down, e.released_at) {
            out = self.release(r);
        }
        if let Some(t) = e.pressed_at {
            *self = Self {
                down_at: Some(t),
                fired: false,
                muted: false,
            };
        }
        self.muted |= muted;
        if let (false, Some(r)) = (e.down, e.released_at) {
            out = self.release(r);
        }
        if e.down
            && !self.fired
            && !self.muted
            && self.down_at.is_some_and(|d| now.since(d) >= HOLD_MS)
        {
            self.fired = true;
            out = Some(Press::Hold);
        }
        out
    }

    fn release(&mut self, t: Ms) -> Option<Press> {
        let d = self.down_at.take()?;
        (!self.fired && !self.muted).then(|| {
            if t.since(d) >= HOLD_MS {
                Press::Hold
            } else {
                Press::Tap
            }
        })
    }
}

pub use chimera_engine::confirm::{Press, Presses};

/// SEQ this frame (ADR 0279), derived once and handed to its gate, the
/// edit path and the band.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeqHold {
    Up,
    /// Down, with no fine turn this frame.
    Held,
    /// Down through a turn on a cell that steps fine: the turn is fine,
    /// and SEQ's press is no key (no tap, no ORBIT hold).
    Turned,
}

impl SeqHold {
    /// SEQ `down`; MIX `mix` (its chord wins: MIX + SEQ learns); a turn
    /// this frame on a cell that steps fine, `fine_turn`.
    pub const fn of(down: bool, mix: bool, fine_turn: bool) -> Self {
        match (down, !mix && fine_turn) {
            (false, _) => SeqHold::Up,
            (true, false) => SeqHold::Held,
            (true, true) => SeqHold::Turned,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct HoldGates {
    menu: HoldGate,
    seq: HoldGate,
    rings: [HoldGate; RINGS],
}

impl Default for HoldGates {
    fn default() -> Self {
        Self::new()
    }
}

impl HoldGates {
    pub const fn new() -> Self {
        Self {
            menu: HoldGate::new(),
            seq: HoldGate::new(),
            rings: [HoldGate::new(); RINGS],
        }
    }

    /// MENU, SEQ and B1–B5 are muted while MIX is down (MIX + SEQ learns,
    /// MIX + B*n* is a mute in ORBIT), SEQ too by a fine turn while it is
    /// down (`SeqHold::Turned`), and B1–B5 while EDIT is: EDIT + B*n*
    /// is no ring key. MIX is read as a level at frame end, so a chord
    /// inside one frame is not muted. The B gates run everywhere, so they are in step
    /// when ORBIT opens.
    pub fn step(&mut self, c: &impl Controls, seq: SeqHold) -> Presses {
        let now = c.now_ms();
        let mix = c.edges(ButtonId::Mix).down;
        let edit = c.edges(ButtonId::Edit).down;
        let mut ring = [None; RINGS];
        for ((g, out), &b) in self.rings.iter_mut().zip(&mut ring).zip(&PART_BUTTONS) {
            *out = g.step(c.edges(b), now, mix || edit);
        }
        let chord = mix || PART_BUTTONS.iter().any(|&b| c.edges(b).down);
        Presses {
            edit: c.edges(ButtonId::Edit).pressed_at.is_some() && !chord,
            menu: self.menu.step(c.edges(ButtonId::Menu), now, mix),
            seq: self
                .seq
                .step(c.edges(ButtonId::Seq), now, mix || seq == SeqHold::Turned),
            ring,
        }
    }
}
