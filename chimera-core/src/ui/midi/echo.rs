//! Value echo out of DIN and USB (spec §§ 7, 9): pulled, not pushed. Each
//! pass every feedback binding's param, as its control would send it, is
//! compared with what it last sent; a difference is sent on the Part's
//! channel, the latest value only, whole messages while DIN's ring has
//! room, and each copied to USB's. The identity reply goes first, out of
//! the port that asked, then the active Part, the other Parts, and last
//! the bursts (a load, a Part change, a map change). A copy USB lost goes
//! again, to USB alone (#842). Bank 2's, as `ui::midi` is; the state is
//! `MidiSlot`'s, in AXI.

use chimera_hal::Ms;
use chimera_midi::ctl::{DataValue, FreeCc};
use chimera_midi::out::{Running, Writer};
use chimera_midi::sysex::identity_reply;
use chimera_midi::take::Place;
use chimera_midi::value::{U7, U14};

use super::out::{ID, REPLY_LEN};
use super::{BINDS, bind_at, span_of, stored};
use crate::MidiChannel;
use crate::block::{Motion, ParamSpec};
use crate::hw::MAX_PARTS;
use crate::midi_in::Port;
use crate::midi_map::{Binding, CcKind, Feedback, Input, NrpnKind};
use crate::out_ring::{OutPorts, OutRing, Tee};
use crate::project::{Loads, PartId, Project};

/// After a binding hears, its own echo waits this long (spec § 9, the
/// owner's call): a ramp, at most 120 ms, lands first.
pub const QUIET_MS: u32 = 250;

/// The longest echo: an NRPN with its fine value, status included.
const MSG_MAX: usize = 9;

/// What each binding last sent (`None`: not since a burst), when it last
/// heard its control (its quiet: a stamp and `since`, so the clock's wrap
/// never holds an echo), the loads and active Part the last pass saw, and
/// whether an identity reply waits, a flag a `Port`. ~1 KB, in `MidiUi`.
#[derive(Clone, Copy)]
pub struct EchoPlan {
    last: [[Option<u16>; BINDS]; MAX_PARTS],
    heard_at: [[Option<Ms>; BINDS]; MAX_PARTS],
    /// Each binding's last echo, out of either port: heard back as turns
    /// within `QUIET_MS`, through a thru loop, it is not a turn (#817).
    echoed: [[Echoed; BINDS]; MAX_PARTS],
    loads: [Loads; MAX_PARTS],
    active: Option<PartId>,
    identity: [bool; Port::COUNT],
    /// Entries (a bit each) whose values USB is owed: a copy was lost,
    /// and USB's poll drops all it holds, ring and packets alike, so
    /// every Part's (#842). Sent USB's alone once the ring is empty and
    /// the loss taken.
    owed: [u32; MAX_PARTS],
}

const _: () = assert!(BINDS <= 32);

/// Each Part's controller layer (ADR 0226), beside its Sound's map. A
/// plain reference, never a `dyn Fn`: a closure's anonymous vtable would
/// land in bank 1.
pub type Layers<'a> = &'a super::layer::Layers;

/// An echo sent: when, and its value (`level`'s). `NONE`'s level is past
/// any a control sends (14 bits at most), so no `Option`: 8 B a slot, not
/// 12, across 32 slots a Part.
#[derive(Clone, Copy)]
struct Echoed {
    at: Ms,
    level: u16,
}

impl Echoed {
    const NONE: Self = Self {
        at: Ms(0),
        level: u16::MAX,
    };
}

/// Nothing sent, built at `p`: a few stores, not a 4 KB static's image
/// (ADR 0251).
///
/// # Safety
/// `p` is valid for a write of `EchoPlan`.
#[inline(never)]
pub(super) unsafe fn silent_at(p: *mut EchoPlan) {
    use super::fill;
    use core::ptr::addr_of_mut;
    const N: usize = BINDS * MAX_PARTS;
    // SAFETY: each field of the caller's `*p`, written whole; a nested
    // array's elements lie in a row.
    unsafe {
        fill(addr_of_mut!((*p).last).cast::<Option<u16>>(), N, None);
        fill(addr_of_mut!((*p).heard_at).cast::<Option<Ms>>(), N, None);
        fill(addr_of_mut!((*p).echoed).cast::<Echoed>(), N, Echoed::NONE);
        fill(
            addr_of_mut!((*p).loads).cast::<Loads>(),
            MAX_PARTS,
            Loads::NONE,
        );
        addr_of_mut!((*p).active).write(None);
        addr_of_mut!((*p).identity).write([false; Port::COUNT]);
        addr_of_mut!((*p).owed).write([0; MAX_PARTS]);
    }
}

impl EchoPlan {
    /// A Device Inquiry was heard on `port`: one reply out of it, however
    /// many asked this pass.
    pub fn ask(&mut self, port: Port) {
        self.identity[port as usize] = true;
    }

    /// MIDI CONFIG › ECHO is off: nothing goes out, and an inquiry heard
    /// meanwhile is never answered.
    pub fn silence(&mut self) {
        self.identity = [false; Port::COUNT];
    }

    /// Every feedback binding of Part `p` sends again: a Sound or project
    /// load, a Part change, a map change.
    pub fn burst(&mut self, p: PartId) {
        self.last[p.index()] = [None; BINDS];
    }

    /// Entry `j` of Part `p` heard its control at `now`: its echo waits
    /// `QUIET_MS`, and what it put the param to (`sent`, the control's
    /// resolution) counts as sent. A relative turn puts no place: its
    /// value goes out once the quiet ends.
    pub fn heard(&mut self, p: PartId, j: usize, sent: Option<u16>, now: Ms) {
        let i = p.index();
        if j >= BINDS {
            return;
        }
        self.heard_at[i][j] = Some(now);
        if sent.is_some() {
            self.last[i][j] = sent;
        }
    }

    /// Entry `j` of Part `p` hears `v` at `now`: its last echo's value,
    /// sent within `QUIET_MS`, so its own echo come back through a thru
    /// loop (#817), whatever moved the param since; a relative binding
    /// takes it for no turn. A real turn of that same byte in that window
    /// is lost with it.
    pub fn looped(&self, p: PartId, j: usize, v: u16, now: Ms) -> bool {
        let sent = self.echoed[p.index()].get(j);
        sent.is_some_and(|e| e.level == v && v != Echoed::NONE.level && now.since(e.at) < QUIET_MS)
    }

    /// One pass (spec §§ 7, 9): the identity replies, each out of the
    /// port that asked, then the active Part `at`'s changed bindings, the
    /// other Parts', then the bursts, each out of every port (`Tee`) while
    /// DIN's ring has room; the rest wait for a later pass.
    pub fn pump(&mut self, project: &Project, ls: Layers, at: PartId, now: Ms, out: &OutPorts) {
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
        if let Some(u) = out.usb
            && u.is_empty()
            && !u.losing()
        {
            self.resend(project, ls, now, u);
        }
        let mut tee = out.tee();
        self.send(project, ls, at, now, &mut tee);
        // A loss, this pass's or one not yet taken: USB's poll drops all
        // it holds.
        if tee.lost() || out.usb.is_some_and(OutRing::losing) {
            self.owed = [u32::MAX; MAX_PARTS];
        }
    }

    /// What USB is owed (`owed`), into its ring alone while it has room.
    fn resend(&mut self, project: &Project, ls: Layers, now: Ms, usb: &OutRing) {
        let mut run = Running::NONE;
        let parts = PartId::ALL
            .into_iter()
            .zip(&mut self.owed)
            .zip(self.heard_at.iter().zip(&mut self.echoed));
        for ((p, owed), heard) in parts {
            let part = project.part(p);
            let layer = ls.layer(p.index());
            for (j, t) in heard.0.iter().enumerate() {
                let bit = 1u32 << j;
                if *owed & bit == 0 || t.is_some_and(|t| now.since(t) < QUIET_MS) {
                    continue;
                }
                let mut b = [0u8; MSG_MAX];
                let mut w = Writer::after(&mut b, run);
                let sent = bind_at(&part.sound.map, layer, j).and_then(|e| {
                    let v = value(project, ls, p, &e)?;
                    render(&mut w, part.mix.channel, &e.input, v).then_some(v)
                });
                let (n, r) = (w.len(), w.running());
                if let (Some(m), Some(level)) = (b.get(..n), sent) {
                    if !usb.push_msg(m) {
                        return;
                    }
                    run = r;
                    heard.1[j] = Echoed { at: now, level };
                }
                *owed &= !bit;
            }
        }
    }

    /// `pump`'s messages through `tee`.
    #[inline(always)]
    fn send(&mut self, project: &Project, ls: Layers, at: PartId, now: Ms, tee: &mut Tee) {
        if self.identity != [false; Port::COUNT] {
            let mut b = [0u8; REPLY_LEN];
            let mut w = Writer::new(&mut b);
            // Our device byte is 0: the parser hears 7F or 0 (`midi_in`).
            let ok = identity_reply(U7::ZERO, &ID, &mut w);
            let n = w.len();
            if ok {
                for p in Port::ALL {
                    let asked = &mut self.identity[p as usize];
                    if *asked && tee.send_to(p, &b[..n]) {
                        *asked = false;
                    }
                }
                // DIN's had no room: it waits for a later pass.
                if self.identity[Port::Din as usize] {
                    return;
                }
            }
            self.identity = [false; Port::COUNT];
        }
        let mut run = Running::NONE;
        for burst in [false, true] {
            let others = PartId::ALL.into_iter().filter(|&p| p != at);
            for p in core::iter::once(at).chain(others) {
                let part = project.part(p);
                let ch = part.mix.channel;
                let layer = ls.layer(p.index());
                for j in 0..BINDS {
                    let Some(e) = bind_at(&part.sound.map, layer, j) else {
                        continue;
                    };
                    let e = &e;
                    if self.last[p.index()][j].is_none() != burst {
                        continue;
                    }
                    let Some(v) = self.due(project, ls, p, j, e, now) else {
                        continue;
                    };
                    let mut b = [0u8; MSG_MAX];
                    let mut w = Writer::after(&mut b, run);
                    let ok = render(&mut w, ch, &e.input, v);
                    let (n, r) = (w.len(), w.running());
                    if !ok {
                        continue;
                    }
                    if !tee.send(&b[..n]) {
                        return;
                    }
                    run = r;
                    self.last[p.index()][j] = Some(v);
                    self.echoed[p.index()][j] = Echoed { at: now, level: v };
                }
            }
        }
    }

    /// Entry `j`, `e`, of Part `p`'s value to send now (spec § 9): not in
    /// its quiet, its `value`, changed.
    fn due(
        &self,
        project: &Project,
        ls: Layers,
        p: PartId,
        j: usize,
        e: &Binding,
        now: Ms,
    ) -> Option<u16> {
        if self.heard_at[p.index()][j].is_some_and(|t| now.since(t) < QUIET_MS) {
            return None;
        }
        let v = value(project, ls, p, e)?;
        (self.last[p.index()][j] != Some(v)).then_some(v)
    }
}

/// Binding `e` of Part `p`'s value as its control sends it, if it echoes:
/// its echo on, its target read by the Part's engine, and no lower Part
/// on the channel echoing the same control.
fn value(project: &Project, ls: Layers, p: PartId, e: &Binding) -> Option<u16> {
    let i = p.index();
    let part = project.part(p);
    if e.feedback != Feedback::Value || !e.target.live(part.sound.params.engine()) {
        return None;
    }
    let a = e.target.addr();
    let spec = a.spec()?;
    let place = param_place(
        spec,
        stored(project, p, a)?,
        super::bins_of(project, a, spec),
    );
    let v = level(&e.input, span_of(e.span, place))?;
    let lower = PartId::ALL[..i].iter().any(|&q| {
        let o = project.part(q);
        o.mix.channel == part.mix.channel
            && (0..BINDS).any(|j| {
                bind_at(&o.sound.map, ls.layer(q.index()), j).is_some_and(|b| {
                    b.input.same_control(&e.input)
                        && b.feedback == Feedback::Value
                        && b.target.live(o.sound.params.engine())
                })
            })
    });
    (!lower).then_some(v)
}

/// The place 0..1 a param at `was` sits at, as a binding sees it: a
/// stepped one at the centre of its bin of `steps` (`cc_bound`'s).
#[inline(always)]
pub(super) fn param_place(spec: &ParamSpec, was: f32, steps: u16) -> f32 {
    match spec.motion() {
        Motion::Stepped(_) => {
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
