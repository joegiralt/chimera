//! Takeover: how an absolute control meets a param that moved without it.

use crate::ctl::Dir;
use crate::value::U7;

/// A param's normalised place, 0..=16383: the unit every absolute input works in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Place(u16);

impl Place {
    pub const ZERO: Place = Place(0);
    pub const MAX: Place = Place(16383);

    pub const fn new(v: u16) -> Option<Self> {
        if v <= 16383 { Some(Self(v)) } else { None }
    }

    /// `v << 7 | v`: 0 → 0 and 127 → 16383.
    pub const fn from_u7_replicated(v: U7) -> Self {
        let v = v.get() as u16;
        Self(v << 7 | v)
    }

    pub const fn get(self) -> u16 {
        self.0
    }
}

/// Pickup tolerance for a 7-bit sender: 2/127 of the span in `Place` units.
pub const TOL_7BIT: u16 = 2 * 129;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Takeover {
    Jump,
    Pickup,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    Move(Place),
    /// Pickup not engaged: turn `toward` the param. Drives the pickup hint.
    Waiting {
        toward: Dir,
    },
}

/// One binding's takeover memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TakeState {
    last_in: Option<Place>,
    engaged: bool,
}

impl TakeState {
    pub const fn new() -> Self {
        Self {
            last_in: None,
            engaged: false,
        }
    }

    /// Something else moved the param: pickup must catch it again.
    pub fn reset(&mut self) {
        self.engaged = false;
    }
}

/// `param` is the param's current place; `tol` is in `Place` units.
pub fn take(t: Takeover, s: &mut TakeState, incoming: Place, param: Place, tol: u16) -> Taken {
    let last = s.last_in.replace(incoming);
    match t {
        Takeover::Jump => Taken::Move(incoming),
        Takeover::Pickup => {
            let crossed =
                last.is_some_and(|l| l.min(incoming) <= param && param <= l.max(incoming));
            if s.engaged || incoming.0.abs_diff(param.0) <= tol || crossed {
                s.engaged = true;
                Taken::Move(incoming)
            } else {
                let toward = if param > incoming { Dir::Up } else { Dir::Down };
                Taken::Waiting { toward }
            }
        }
        Takeover::Scale => Taken::Move(scale(last, incoming, param)),
    }
}

/// Ableton's Value Scaling: up, p' = p + (c'-c)(1-p)/(1-c); down,
/// p' = p - (c-c')p/c. Both ends converge. No previous input: no move.
fn scale(last: Option<Place>, incoming: Place, param: Place) -> Place {
    const M: u32 = 16383;
    let (c, c2, p) = match last {
        Some(l) => (l.0 as u32, incoming.0 as u32, param.0 as u32),
        None => return param,
    };
    let p2 = if c2 > c {
        p + (c2 - c) * (M - p) / (M - c)
    } else if c2 < c {
        p - (c - c2) * p / c
    } else {
        p
    };
    Place(p2 as u16)
}
