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

    /// One result per frame; in a stalled frame the old press's release
    /// wins, and the new press's hold fires the frame after (#289).
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
        if out.is_none()
            && e.down
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

/// SEQ's and MIX's keys this frame, before the turns (ADR 0279).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeqKey {
    Up,
    Down,
    /// Down with MIX: MIX + SEQ's chord (learn) wins.
    UnderMix,
}

impl SeqKey {
    pub const fn of(seq: Edges, mix: Edges) -> Self {
        match (seq.down, mix.down) {
            (false, _) => SeqKey::Up,
            (true, false) => SeqKey::Down,
            (true, true) => SeqKey::UnderMix,
        }
    }
}

/// What this frame's turns did to SEQ's hold (ADR 0279).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeqTurn {
    Untouched,
    /// A turn on a Part's, mixer's or FX page, SEQ down: the owner's
    /// ruling, a turn during a SEQ hold cancels its tap and ORBIT.
    Turned,
}

/// SEQ this frame (ADR 0279), derived once and handed to its gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeqHold {
    Up,
    /// Down, with no turn this frame that cancels it.
    Held,
    /// Down through a turn: SEQ's press is no key (no tap, no ORBIT
    /// hold).
    Turned,
}

impl SeqHold {
    pub const fn of(key: SeqKey, turn: SeqTurn) -> Self {
        match (key, turn) {
            (SeqKey::Up, _) => SeqHold::Up,
            (SeqKey::Down, SeqTurn::Turned) => SeqHold::Turned,
            (SeqKey::Down | SeqKey::UnderMix, _) => SeqHold::Held,
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
    /// MIX + B*n* is a mute in ORBIT), SEQ too by a turn while it is
    /// down (`SeqHold::Turned`), and B1–B5 while EDIT is: EDIT + B*n*
    /// is no ring key. MIX is down at frame end, or was through the key's
    /// press inside the frame (`chorded`, #289). The B gates run
    /// everywhere, so they are in step when ORBIT opens.
    pub fn step(&mut self, c: &impl Controls, seq: SeqHold) -> Presses {
        let now = c.now_ms();
        let m = c.edges(ButtonId::Mix);
        let mix = |e: Edges| m.down || chorded(m, e);
        let edit = c.edges(ButtonId::Edit).down;
        let mut ring = [None; RINGS];
        for ((g, out), &b) in self.rings.iter_mut().zip(&mut ring).zip(&PART_BUTTONS) {
            let e = c.edges(b);
            *out = g.step(e, now, mix(e) || edit);
        }
        let chord = m.down || PART_BUTTONS.iter().any(|&b| c.edges(b).down);
        let (menu, seq_e) = (c.edges(ButtonId::Menu), c.edges(ButtonId::Seq));
        Presses {
            edit: c.edges(ButtonId::Edit).pressed_at.is_some() && !chord,
            menu: self.menu.step(menu, now, mix(menu)),
            seq: self
                .seq
                .step(seq_e, now, mix(seq_e) || seq == SeqHold::Turned),
            ring,
        }
    }
}

/// MIX down at the press in `e` and let go after it inside this frame,
/// pressed in it or held from before: a chord the level at frame end
/// misses (#289).
fn chorded(mix: Edges, e: Edges) -> bool {
    match (mix.pressed_at, mix.released_at, e.pressed_at) {
        (down, Some(up), Some(p)) => {
            down.is_none_or(|d| p.signed_since(d) >= 0) && up.signed_since(p) >= 0
        }
        (_, None, _) | (_, _, None) => false,
    }
}
