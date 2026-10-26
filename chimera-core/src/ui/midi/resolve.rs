//! What one control message does to its binding's param (spec § 6.2):
//! pure, over the binding, its runtime and the param's place now. The
//! caller scales a `Place` into the span and ramps or bins it, and turns
//! `Detents` through the encoder's edit path.

use chimera_midi::ctl::{Ctl, DataValue, Dir};
use chimera_midi::law::{ValueLaw, place_of};
use chimera_midi::take::{Place, TOL_7BIT, TakeState, Taken, Takeover, take};
use chimera_midi::value::U7;

use crate::midi_map::{Binding, ButtonMode, CcKind, Input, NrpnKind};

/// One entry's state between messages: never stored, reset on a load or a
/// map change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindRuntime {
    take: TakeState,
    /// Where this binding last put its param, in the controller's places:
    /// the param anywhere else means another hand moved it, and pickup
    /// waits again.
    wrote: Option<Place>,
    /// A 14-bit pair's last MSB.
    msb14: Option<U7>,
    /// A button's last state: held.
    held: bool,
    /// A toggle's: at `span.max`.
    toggled: bool,
    /// A pickup waiting, and which way the knob must go: the hint's.
    waiting: Option<Dir>,
}

impl BindRuntime {
    pub const NEW: Self = Self {
        take: TakeState::new(),
        wrote: None,
        msb14: None,
        held: false,
        toggled: false,
        waiting: None,
    };

    /// A pickup waiting for the knob, and which way it must move.
    pub fn waiting(&self) -> Option<Dir> {
        self.waiting
    }

    /// Where this binding last put its param, if nothing moved it since.
    pub fn wrote(&self) -> Option<Place> {
        self.wrote
    }
}

impl Default for BindRuntime {
    fn default() -> Self {
        Self::NEW
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// The param to this place of its span (`Place::MAX` is `span.max`).
    Place(Place),
    /// Panel detents, through the encoder's edit path.
    Detents(i16),
    /// A pickup not yet engaged: the knob must move this way.
    Wait(Dir),
}

/// What `c` does to `b`'s param, now at place `now` of the span (the
/// controller's places). `None`: nothing (a repeat, a lone LSB, a release).
/// `c` must be one `b.input` hears (`Input::heard`).
pub fn resolve(b: &Binding, rt: &mut BindRuntime, c: &Ctl, now: Place) -> Option<Action> {
    // Moved by anything but this binding since: pickup waits again.
    if rt
        .wrote
        .is_some_and(|w| w.get().abs_diff(now.get()) > TOL_7BIT)
    {
        rt.take.reset();
        rt.wrote = None;
    }
    match (b.input, *c) {
        (Input::Cc(_, k), Ctl::Cc { value, .. }) => match k {
            CcKind::Absolute(t) => absolute(rt, t, Place::from_u7_replicated(value), now),
            CcKind::Relative(e) => detents(e.decode(value)),
            CcKind::Button(m) => button(rt, m, value),
        },
        (Input::Cc14(m, t), Ctl::Cc { cc, value }) => {
            if cc.get() == m.get() {
                rt.msb14 = Some(value);
                absolute(rt, t, Place::from_u7_replicated(value), now)
            } else {
                let msb = rt.msb14?;
                let p = Place::new(u16::from(msb.get()) << 7 | u16::from(value.get()))?;
                absolute(rt, t, p, now)
            }
        }
        (Input::Nrpn(_, k), Ctl::Data { value, .. }) => match k {
            NrpnKind::Absolute(t) => {
                let p = place_of(ValueLaw::Place14, value, 0, |_| None)?;
                absolute(rt, t, p, now)
            }
            NrpnKind::Relative(e) => match value {
                DataValue::Coarse(v) => detents(e.decode(v)),
                DataValue::Fine(_) => None,
            },
            NrpnKind::Button(m) => button(rt, m, msb(value)),
            NrpnKind::IncDec => None,
        },
        (Input::Nrpn(_, NrpnKind::IncDec), Ctl::Step { dir, .. }) => {
            Some(Action::Detents(if dir == Dir::Up { 1 } else { -1 }))
        }
        (Input::Pressure(t), Ctl::Pressure(v)) => {
            absolute(rt, t, Place::from_u7_replicated(v), now)
        }
        _ => None,
    }
}

#[inline(always)]
fn msb(v: DataValue) -> U7 {
    match v {
        DataValue::Coarse(m) => m,
        DataValue::Fine(f) => f.msb(),
    }
}

#[inline(always)]
fn detents(d: i8) -> Option<Action> {
    (d != 0).then_some(Action::Detents(i16::from(d)))
}

/// `incoming` through takeover `t`. Scale's first message is no move.
fn absolute(rt: &mut BindRuntime, t: Takeover, incoming: Place, now: Place) -> Option<Action> {
    match take(t, &mut rt.take, incoming, now, TOL_7BIT) {
        Taken::Move(p) => {
            rt.waiting = None;
            rt.wrote = Some(p);
            (t != Takeover::Scale || p != now).then_some(Action::Place(p))
        }
        Taken::Waiting { toward } => {
            rt.waiting = Some(toward);
            Some(Action::Wait(toward))
        }
    }
}

/// Held is max (momentary); a press flips (toggle). `v` ≥ 64 is held.
fn button(rt: &mut BindRuntime, m: ButtonMode, v: U7) -> Option<Action> {
    let held = v.get() >= 64;
    let was = core::mem::replace(&mut rt.held, held);
    let up = match m {
        ButtonMode::Momentary if held != was || rt.wrote.is_none() => held,
        ButtonMode::Toggle if held && !was => {
            rt.toggled = !rt.toggled;
            rt.toggled
        }
        _ => return None,
    };
    let p = if up { Place::MAX } else { Place::ZERO };
    rt.wrote = Some(p);
    Some(Action::Place(p))
}
