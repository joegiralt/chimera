//! Value echo out of DIN (spec § 9): pulled, not pushed. Each pass every
//! feedback binding's param, as its control would send it, is compared
//! with what it last sent; a difference is sent on the Part's channel, the
//! latest value only, whole messages while the out ring has room. The
//! identity reply goes first, then the active Part, the other Parts, and
//! last the bursts (a load, a Part change, a map change). Bank 2's, as
//! `ui::midi` is; the state is `MidiSlot`'s, in AXI.

use chimera_hal::Ms;
use chimera_midi::ctl::{DataValue, FreeCc};
use chimera_midi::out::{Running, Writer};
use chimera_midi::sysex::identity_reply;
use chimera_midi::take::Place;
use chimera_midi::value::{U7, U14};

use super::out::{ID, REPLY_LEN};
use super::{span_of, stored};
use crate::MidiChannel;
use crate::block::{Motion, ParamSpec};
use crate::hw::MAX_PARTS;
use crate::midi_map::{Binding, CcKind, Feedback, Input, MAP_LEN, NrpnKind};
use crate::out_ring::OutRing;
use crate::project::{Loads, PartId, Project};

/// After a binding hears, its own echo waits this long (spec § 9, the
/// owner's call): a ramp, at most 120 ms, lands first.
pub const QUIET_MS: u32 = 250;

/// The longest echo: an NRPN with its fine value, status included.
const MSG_MAX: usize = 9;

/// What each binding last sent (`None`: not since a burst), when it last
/// heard its control (its quiet: a stamp and `since`, so the clock's wrap
/// never holds an echo), the loads and active Part the last pass saw, and
/// whether an identity reply waits. ~1 KB, in `MidiUi`.
#[derive(Clone, Copy)]
pub struct EchoPlan {
    last: [[Option<u16>; MAP_LEN]; MAX_PARTS],
    heard_at: [[Option<Ms>; MAP_LEN]; MAX_PARTS],
    loads: [Loads; MAX_PARTS],
    active: Option<PartId>,
    identity: bool,
}

/// Nothing sent: a named static, so its image is bank 2's.
pub(super) static SILENT: EchoPlan = EchoPlan {
    last: [[None; MAP_LEN]; MAX_PARTS],
    heard_at: [[None; MAP_LEN]; MAX_PARTS],
    loads: [Loads::NONE; MAX_PARTS],
    active: None,
    identity: false,
};

impl EchoPlan {
    /// A Device Inquiry was heard: one reply, however many asked this pass.
    pub fn ask(&mut self) {
        self.identity = true;
    }

    /// MIDI CONFIG › ECHO is off: nothing goes out, and an inquiry heard
    /// meanwhile is never answered.
    pub fn silence(&mut self) {
        self.identity = false;
    }

    /// Every feedback binding of Part `p` sends again: a Sound or project
    /// load, a Part change, a map change.
    pub fn burst(&mut self, p: PartId) {
        self.last[p.index()] = [None; MAP_LEN];
    }

    /// Entry `j` of Part `p` heard its control at `now`: its echo waits
    /// `QUIET_MS`, and what it put the param to (`sent`, the control's
    /// resolution) counts as sent. A relative turn puts no place: its
    /// value goes out once the quiet ends.
    pub fn heard(&mut self, p: PartId, j: usize, sent: Option<u16>, now: Ms) {
        let i = p.index();
        if j >= MAP_LEN {
            return;
        }
        self.heard_at[i][j] = Some(now);
        if sent.is_some() {
            self.last[i][j] = sent;
        }
    }

    /// One pass (spec § 9): the identity reply, then the active Part `at`'s
    /// changed bindings, the other Parts', then the bursts, each whole into
    /// `ring` while it has room; the rest wait for a later pass.
    pub fn pump(&mut self, project: &Project, at: PartId, now: Ms, ring: &OutRing) {
        if self.active != Some(at) {
            self.active = Some(at);
            self.burst(at);
        }
        for p in PartId::ALL {
            let l = project.part(p).loads();
            if l != self.loads[p.index()] {
                self.loads[p.index()] = l;
                self.burst(p);
            }
        }
        if self.identity {
            let mut b = [0u8; REPLY_LEN];
            let mut w = Writer::new(&mut b);
            // Our device byte is 0: the parser hears 7F or 0 (`midi_in`).
            let ok = identity_reply(U7::ZERO, &ID, &mut w);
            let n = w.len();
            if ok && !ring.push_msg(&b[..n]) {
                return;
            }
            self.identity = false;
        }
        let mut run = Running::NONE;
        for burst in [false, true] {
            let others = PartId::ALL.into_iter().filter(|&p| p != at);
            for p in core::iter::once(at).chain(others) {
                let part = project.part(p);
                let ch = part.mix.channel;
                for (j, e) in part.sound.map.entries().iter().enumerate() {
                    if self.last[p.index()][j].is_none() != burst {
                        continue;
                    }
                    let Some(v) = self.due(project, p, j, e, now) else {
                        continue;
                    };
                    let mut b = [0u8; MSG_MAX];
                    let mut w = Writer::after(&mut b, run);
                    let ok = render(&mut w, ch, &e.input, v);
                    let (n, r) = (w.len(), w.running());
                    if !ok {
                        continue;
                    }
                    if !ring.push_msg(&b[..n]) {
                        return;
                    }
                    run = r;
                    self.last[p.index()][j] = Some(v);
                }
            }
        }
    }

    /// Entry `j`, `e`, of Part `p`'s value to send now, if it echoes and
    /// changed: its echo on, its target read by the Part's engine, not in
    /// its quiet, and no lower Part on the channel echoing the same
    /// control (spec § 9).
    fn due(&self, project: &Project, p: PartId, j: usize, e: &Binding, now: Ms) -> Option<u16> {
        let i = p.index();
        let part = project.part(p);
        if e.feedback != Feedback::Value || !e.target.live(part.sound.params.engine()) {
            return None;
        }
        if self.heard_at[i][j].is_some_and(|t| now.since(t) < QUIET_MS) {
            return None;
        }
        let a = e.target.addr();
        let place = param_place(a.spec()?, stored(project, p, a)?);
        let v = level(&e.input, span_of(e.span, place))?;
        if self.last[i][j] == Some(v) {
            return None;
        }
        let lower = PartId::ALL[..i].iter().any(|&q| {
            let o = project.part(q);
            o.mix.channel == part.mix.channel
                && o.sound
                    .map
                    .targets_of(&e.input)
                    .any(|(_, b)| b.feedback == Feedback::Value)
        });
        (!lower).then_some(v)
    }
}

/// The place 0..1 a param at `was` sits at, as a binding sees it: a
/// stepped one at its bin's centre (`cc_bound`'s).
#[inline(always)]
pub(super) fn param_place(spec: &ParamSpec, was: f32) -> f32 {
    match spec.motion() {
        Motion::Stepped(_) => {
            let steps = (spec.max - spec.min) as u16 + 1;
            let at = (was - spec.min + 0.5) as u16;
            ((f32::from(at) + 0.5) * 256.0 / f32::from(steps)).min(255.0) / 255.0
        }
        Motion::Continuous(_) => spec.normalize(was),
    }
}

/// Place `p` of a binding's span as its control sends it: 7 bits for a CC
/// (relative ones too: the LED ring's value), rounded, so a CC heard as
/// `v` reads back `v`; 0 or 127 for a button; 14 bits for a 14-bit pair or
/// an NRPN. `None` for pressure: no echo.
#[inline(always)]
pub fn level(input: &Input, p: Place) -> Option<u16> {
    let button = || if p.get() >= 8192 { 127 } else { 0 };
    match input {
        Input::Cc(_, CcKind::Button(_)) | Input::Nrpn(_, NrpnKind::Button(_)) => Some(button()),
        Input::Cc(..) => Some(((u32::from(p.get()) * 127 + 8191) / 16383) as u16),
        Input::Cc14(..) | Input::Nrpn(..) => Some(p.get()),
        Input::Pressure(_) => None,
    }
}

/// `v` (`level`'s) on `input`'s control and encoding, on channel `ch`:
/// a CC, a 14-bit pair MSB then LSB, an NRPN 99, 98, 6 and 38 (a button's
/// 6 alone). Whole or not at all.
fn render(w: &mut Writer, ch: MidiChannel, input: &Input, v: u16) -> bool {
    let u7 = || U7::new(v.min(127) as u8);
    match *input {
        Input::Cc(c, _) => match (FreeCc::new(c.get()), u7()) {
            (Some(cc), Some(x)) => w.cc(ch, cc, x),
            _ => false,
        },
        Input::Cc14(m, _) => U14::new(v).is_some_and(|x| w.cc14(ch, m, x)),
        Input::Nrpn(n, NrpnKind::Button(_)) => {
            u7().is_some_and(|x| w.nrpn(ch, n, DataValue::Coarse(x)))
        }
        Input::Nrpn(n, _) => U14::new(v).is_some_and(|x| w.nrpn(ch, n, DataValue::Fine(x))),
        Input::Pressure(_) => false,
    }
}
