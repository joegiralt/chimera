//! Tap and hold from latched button edges: a tap acts on release, a hold
//! fires once at `HOLD_MS` while still down.

use chimera_hal::{ButtonId, Controls, Edges, Ms};

pub const HOLD_MS: u32 = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Tap,
    Hold,
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Presses {
    pub menu: Option<Press>,
    pub seq: Option<Press>,
}

#[derive(Clone, Copy, Debug)]
pub struct HoldGates {
    menu: HoldGate,
    seq: HoldGate,
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
        }
    }

    /// MENU is muted while MIX is down; MIX is read as a level at frame end, so a chord inside one frame is not muted.
    pub fn step(&mut self, c: &impl Controls) -> Presses {
        let now = c.now_ms();
        let mix = c.edges(ButtonId::Mix).down;
        Presses {
            menu: self.menu.step(c.edges(ButtonId::Menu), now, mix),
            seq: self.seq.step(c.edges(ButtonId::Seq), now, false),
        }
    }
}
