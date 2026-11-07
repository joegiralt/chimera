//! Bindings move the Sound (spec §§ 5-7, ADR 0217): each Part's map heard
//! on its channel, an absolute place ramped over the gap between messages
//! (or binned, for a stepped target), a relative turn through the
//! encoder's edit path, an NRPN no binding names moving its canonical
//! param, and RPN 0 setting BEND. Only as many knob writes a Part as the
//! moves in flight to the audio leave room for, so the audio never takes
//! more than `MOVES` at once (ADR 0139).
//!
//! Bank 2's, behind `MidiUiDoor` (`ui::cc`, ADR 0198): `bank2.x` links
//! this module there whole. Bank 1 keeps the state's home and the stubs.

// Without `midi-map` the door's `hear` is `ui::cc::deaf`: none of this runs.
#![cfg_attr(not(feature = "midi-map"), allow(dead_code))]

pub mod echo;
pub mod learn;
pub mod out;
#[cfg(feature = "midi-map")]
pub mod page;
pub mod ramp;
pub mod resolve;

use chimera_hal::Ms;
use chimera_midi::ctl::{Ctl, DataValue, Dir, FreeCc, Sel};
use chimera_midi::law::place_of;
use chimera_midi::learn::Seen;
use chimera_midi::take::Place;
use chimera_midi::value::{U7, U14};

use super::UiState;
use super::cc::MidiSlot;
use super::nav::PartChain;
use super::part_page::Edit;
use crate::addr::{Blocks, ParamAddr};
use crate::block::Motion;
use crate::dsp::clampf;
use crate::dsp::knobs::MOVES;
use crate::hw::MAX_PARTS;
use crate::midi_in::{CtrlEvent, HeardEv};
use crate::midi_map::{Binding, MAP_LEN, Mappable, Span};
use crate::nrpn::{self, NrpnAddr};
use crate::part::{MAX_BEND, PartParams};
use crate::project::{Loads, PartId, Project};
use ramp::{Ramp, bin, budget};
use resolve::{Action, BindRuntime, resolve};

/// Canonical NRPN targets a Part moves at once. The rest wait in line, each
/// with its latest place, till a slot comes to rest (spec § 5: delayed as a
/// CC is, not lost); past `WAIT` addresses waiting, the oldest gives way.
const CANON: usize = 8;
/// Canonical targets that wait for a slot, first come first moved.
const WAIT: usize = 16;
/// A Part's slots: its map's entries, then its canonical NRPN targets.
const SLOTS: usize = MAP_LEN + CANON;
const _: () = assert!(SLOTS <= 32, "`handed` holds a bit a slot");
/// No bin heard since a load or the hand: the stored value's.
const NO_BIN: u8 = u8::MAX;
/// RPN 0, pitch-bend sensitivity.
const RPN0: Sel = Sel::Rpn(U14::from_parts(U7::ZERO, U7::ZERO));
const BEND: ParamAddr = ParamAddr::new(crate::addr::BlockRef::Part, PartParams::BEND);

/// A Part's slots' ramps, last bins and waiting bins, its entries'
/// takeover state and detents not yet turned, its canonical targets, its
/// budget cursor, the Sound loads they were heard on and the slots the
/// hand turned this pass.
#[derive(Clone, Copy)]
pub struct PartMidi {
    ramps: [Ramp; SLOTS],
    bins: [u8; SLOTS],
    /// A stepped slot's bin that found no room: it goes first next pass.
    pend: [u8; SLOTS],
    rt: [BindRuntime; MAP_LEN],
    /// Detents heard and not yet turned: one edit an entry a pass.
    turns: [i16; MAP_LEN],
    canon: [Option<Mappable>; CANON],
    /// Canonical targets no slot was free for, and their latest places, in
    /// the order they came: the `None`s after.
    wait: [Option<(Mappable, Place)>; WAIT],
    cursor: u8,
    loads: Loads,
    /// Slots the hand turned this pass: nothing heard moves them till the next.
    handed: u32,
}

/// A Part's slots at rest: a named static, so its image is bank 2's
/// (`bank2.x`), not an anonymous constant left in bank 1.
static REST: PartMidi = PartMidi {
    ramps: [Ramp::rest(0.0); SLOTS],
    bins: [NO_BIN; SLOTS],
    pend: [NO_BIN; SLOTS],
    rt: [BindRuntime::NEW; MAP_LEN],
    turns: [0; MAP_LEN],
    canon: [None; CANON],
    wait: [None; WAIT],
    cursor: 0,
    loads: Loads::NONE,
    handed: 0,
};

impl PartMidi {
    /// At rest: its map changed (a learn, a load), and its slots are
    /// indexed by entry. The loads it was heard on and the hand's turns
    /// this pass stand.
    fn reset(&mut self) {
        let (loads, handed) = (self.loads, self.handed);
        *self = PartMidi {
            loads,
            handed,
            ..REST
        };
    }

    /// Canonical slot `k` is free to move: at rest, no bin waiting, and
    /// not the hand's this pass (its target's message would be refused).
    fn rests(&self, k: usize) -> bool {
        let j = MAP_LEN + k;
        !self.ramps[j].moving() && self.pend[j] == NO_BIN && self.handed >> j & 1 == 0
    }

    /// Where canonical target `t` goes now: its own slot at rest (or still
    /// moving, if `sweep`), else a free one, taken for it. One copy, bank
    /// 2's: inlined twice, its compare is left in bank 1 (ADR 0175).
    #[inline(never)]
    fn seat(&mut self, t: Mappable, sweep: bool) -> Seat {
        if let Some(k) = self.canon.iter().position(|&c| c == Some(t)) {
            return if self.handed >> (MAP_LEN + k) & 1 != 0 {
                Seat::Handed
            } else if sweep || self.rests(k) {
                Seat::Move(k)
            } else {
                Seat::Wait
            };
        }
        let Some(k) = (0..CANON).find(|&k| self.rests(k)) else {
            return Seat::Wait;
        };
        let j = MAP_LEN + k;
        self.canon[k] = Some(t);
        (self.ramps[j], self.bins[j]) = (Ramp::rest(0.0), NO_BIN);
        Seat::Move(k)
    }

    /// `t` waits to move to `to`: its place, if it waits already, else last
    /// in line. A full line's oldest gives way: the newest is the hand's.
    fn wait_for(&mut self, t: Mappable, to: Place) {
        let at = self.wait.iter().position(|w| w.is_none_or(|(u, _)| u == t));
        let i = at.unwrap_or_else(|| {
            self.unwait(0);
            WAIT - 1
        });
        self.wait[i] = Some((t, to));
    }

    /// The wait at `i` leaves the line; the rest close up.
    fn unwait(&mut self, i: usize) {
        for x in i..WAIT - 1 {
            self.wait[x] = self.wait[x + 1];
        }
        self.wait[WAIT - 1] = None;
    }

    /// `a` no longer waits.
    fn unwait_addr(&mut self, a: ParamAddr) {
        let at = self
            .wait
            .iter()
            .position(|w| w.is_some_and(|(u, _)| u.addr() == a));
        if let Some(i) = at {
            self.unwait(i);
        }
    }
}

/// Where a canonical target goes now (`PartMidi::seat`).
enum Seat {
    Move(usize),
    Wait,
    /// The hand turned it this pass: the message is refused (spec § 6.2).
    Handed,
}

/// Each Part's, whether a stepped or relative move edited the active Part
/// this pass, what learn has heard of the control it senses, and DIN OUT's
/// echo plan, the identity reply among it. ~6.6 KB, AXI (`MidiSlot`).
pub struct MidiUi {
    parts: [PartMidi; MAX_PARTS],
    stepped: bool,
    seen: Seen,
    echo: echo::EchoPlan,
}

/// Nothing sensed: a named static, so its image is bank 2's.
static NO_SEEN: Seen = Seen::new();

impl MidiSlot {
    /// The state, built on first use from `REST`: bank 1 never writes it.
    #[inline(always)]
    fn ui(&mut self) -> &mut MidiUi {
        if !self.built {
            let m = self.ui.as_mut_ptr();
            // SAFETY: `m` points at the slot's own, unaliased memory; each
            // field is written whole before `built` says it is.
            unsafe {
                for i in 0..MAX_PARTS {
                    core::ptr::addr_of_mut!((*m).parts[i]).write(REST);
                }
                core::ptr::addr_of_mut!((*m).stepped).write(false);
                core::ptr::addr_of_mut!((*m).seen).write(NO_SEEN);
                core::ptr::addr_of_mut!((*m).echo).write(echo::SILENT);
            }
            self.built = true;
        }
        // SAFETY: written above, or by an earlier call that set `built`;
        // only `clear` unsets it, and the next call writes it again.
        unsafe { self.ui.assume_init_mut() }
    }

    /// The state, if built: a read never builds it.
    #[inline(always)]
    fn built(&self) -> Option<&MidiUi> {
        // SAFETY: `built` says the slot was written (`ui`).
        self.built.then(|| unsafe { self.ui.assume_init_ref() })
    }

    #[inline(always)]
    fn part(&mut self, i: usize) -> &mut PartMidi {
        &mut self.ui().parts[i]
    }

    /// The Parts whose map changed since the door last ran, reset.
    fn fresh(&mut self) {
        let stale = core::mem::take(&mut self.stale);
        for i in 0..MAX_PARTS {
            if stale >> i & 1 != 0 {
                self.part(i).reset();
                self.ui().echo.burst(PartId::ALL[i]);
            }
        }
    }
}

/// Word number `want` of `table`, its words `|` between: the bytes
/// between its `|`s, none past the last. Checked, never failing. Bank 2's
/// word tables are named statics, so their bytes are bank 2's, not
/// anonymous string rodata left in bank 1. Inlined into one out-of-line
/// reader a table: a shared callee's argument set-up is what the machine
/// outliner pulls into bank 1 (ADR 0175).
#[inline(always)]
#[allow(clippy::manual_unwrap_or_default)]
fn pick(table: &'static [u8], want: u8) -> &'static str {
    let (mut n, mut start, mut k) = (0u8, 0usize, 0usize);
    while k <= table.len() {
        if k == table.len() || table[k] == b'|' {
            if n == want {
                // Matches, not `Option::map`: a core instance is bank 1's.
                return match table.get(start..k) {
                    Some(b) => match core::str::from_utf8(b) {
                        Ok(s) => s,
                        Err(_) => "",
                    },
                    None => "",
                };
            }
            n += 1;
            start = k + 1;
        }
        k += 1;
    }
    ""
}

/// `a`'s stored value on Part `p`.
#[inline(always)]
fn stored(project: &Project, p: PartId, a: ParamAddr) -> Option<f32> {
    Some(project.read_part(p).block(a.block)?.get(a.param))
}

/// Glided by the Part's knobs: counted by the publish (`KNOBS`).
#[inline(always)]
fn knob(a: ParamAddr) -> bool {
    a.spec().and_then(|s| s.motion().knob()).is_some()
}

/// Place `p` of `s` as a place 0..1 of the param. For a 7-bit value `v`
/// (`v << 7 | v`) it is bit for bit ADR 0139's `v / 127` place.
#[inline(always)]
fn span_place(s: Span, p: Place) -> f32 {
    let (lo, hi) = (f32::from(s.min), f32::from(s.max));
    (lo + (hi - lo) * (f32::from(p.get()) / 16383.0)) / 255.0
}

/// A param's place 0..1 as a place of `s`, the controller's view of it.
#[inline(always)]
fn span_of(s: Span, place: f32) -> Place {
    let (lo, hi) = (f32::from(s.min), f32::from(s.max));
    if lo == hi {
        return Place::ZERO;
    }
    let x = clampf((place * 255.0 - lo) / (hi - lo), 0.0, 1.0);
    Place::new((x * 16383.0 + 0.5) as u16).unwrap_or(Place::MAX)
}

/// The control message in `e`, its learn heard first; `None` if learn
/// took it or it is no control a binding hears.
fn control(ui: &mut UiState, e: HeardEv) -> Option<(crate::MidiChannel, Ctl)> {
    let (ch, c) = match e {
        HeardEv::Cc(cc) => {
            let Some(free) = FreeCc::new(cc.cc.get()) else {
                ui.learn_reserved(cc);
                return None;
            };
            let value = U7::new(cc.value)?;
            (cc.channel, Ctl::Cc { cc: free, value })
        }
        HeardEv::Ctrl(_, CtrlEvent::Ctl(ch, c)) => (ch, c),
        HeardEv::Ctrl(..) => return None,
    };
    if ui.learn_in(ch, &c) {
        return None;
    }
    Some((ch, c))
}

impl UiState {
    /// Each Part's ramps `now` on, then this pass's control messages (spec
    /// §§ 5-7). After `handle_input`, before the publish: the hand's moves
    /// are in. `taken`: the audio has taken the last publish. A load
    /// waiting drops them (spec § 1): a write now would refuse it or adopt
    /// its witness confirmed. Saves and listings replace nothing (#528).
    /// The door's `hear`.
    pub(super) fn hear_in(&mut self, ev: &mut dyn Iterator<Item = HeardEv>, now: Ms, taken: bool) {
        self.cc.fresh();
        let elapsed = now.since(self.cc.last);
        self.cc.last = now;
        use crate::ui::settings::job::Job;
        let busy = matches!(
            self.job,
            Some(Job::Load(_) | Job::ClearOwn(..) | Job::LibraryLoad(_))
        ) || self.lib.busy()
            || self.src.loading();
        let at = self.active_part;
        let chain = PartChain::of(&self.project.part(at).sound);
        let (before, rev) = (self.ctx(), self.project.rev());
        // The audio takes every move published since it last took one, over
        // however many passes: two can read the ms clock microseconds apart
        // (one a redraw held up). So the room is what the moves in flight,
        // the hand's this pass among them, leave.
        if taken {
            self.cc.flight = [0; MAX_PARTS];
        }
        // Waiting bins first, then ramps: one heard now starts now, not
        // `elapsed` ago.
        for p in PartId::ALL {
            let i = p.index();
            let hand = if p == at { self.cc.hand } else { 0 };
            self.cc.flight[i] = self.cc.flight[i].saturating_add(hand);
            // One kept for a key's edit (a route deleted or primed).
            self.cc.room[i] = (MOVES as u8 - 1).saturating_sub(self.cc.flight[i]);
            if busy {
                continue;
            }
            self.cc_reset_on_load(p);
            self.cc_pending(p);
            if elapsed > 0 {
                self.cc_step(p, elapsed);
            }
            self.cc_waiting(p, now);
        }
        for e in ev {
            // A Device Inquiry asks nothing of the Sound: answered busy or
            // not, two in a pass with one reply (`out`), out of the port
            // that asked.
            if let HeardEv::Ctrl(port, CtrlEvent::Identity { .. }) = e {
                self.cc.ui().echo.ask(port);
                continue;
            }
            if busy {
                continue;
            }
            if let Some((ch, c)) = control(self, e) {
                self.cc_control(ch, &c, now);
            }
        }
        if !busy {
            for p in PartId::ALL {
                self.cc_turns(p);
            }
        }
        for (f, r) in self.cc.flight.iter_mut().zip(self.cc.room) {
            *f = (*f).max(MOVES as u8 - 1) - r;
        }
        let m = self.cc.ui();
        for part in &mut m.parts {
            part.handed = 0;
        }
        // The page follows a stepped move on the active Part as a turn's.
        if core::mem::take(&mut m.stepped) && self.loc.page(&self.cx()).is_some() {
            self.after_edit(at, chain, before, self.page_def());
        }
        // A move on a browsed Part moves its cost and meter hints.
        self.browser_dirty |= self.loc.browse().is_some() && self.project.rev() != rev;
    }

    /// One message on channel `ch`: RPN 0, then each Part's bindings on the
    /// channel, or the canonical param of an NRPN none of them names.
    fn cc_control(&mut self, ch: crate::MidiChannel, c: &Ctl, now: Ms) {
        for p in PartId::ALL {
            if self.project.part(p).mix.channel != ch {
                continue;
            }
            match *c {
                Ctl::Data { sel, value } if sel == RPN0 => self.cc_bend(p, Some(value), None),
                Ctl::Step { sel, dir } if sel == RPN0 => self.cc_bend(p, None, Some(dir)),
                _ => {}
            }
            let (i, map) = (p.index(), self.project.part(p).sound.map);
            let handed = self.cc.part(i).handed;
            let mut bound = false;
            for (j, e) in map.entries().iter().enumerate() {
                if !e.input.heard(c) {
                    continue;
                }
                bound = true;
                // The hand beats a message in its pass (spec § 6.2).
                if handed >> j & 1 == 0 {
                    self.cc_bound(p, j, *e, c, now);
                }
            }
            if let (
                false,
                Ctl::Data {
                    sel: Sel::Nrpn(n),
                    value,
                },
            ) = (bound, *c)
            {
                self.cc_canonical(p, n, value, now);
            }
        }
    }

    /// RPN 0 on Part `p` (spec § 5): BEND to `Coarse(m)` semitones, clamped
    /// to `MAX_BEND`, or a step of one, through the Part's edit path.
    fn cc_bend(&mut self, p: PartId, to: Option<DataValue>, step: Option<Dir>) {
        let i = p.index();
        let cost = u8::from(knob(BEND));
        if cost > self.cc.room[i] {
            return;
        }
        let moved = match (to, step) {
            (Some(DataValue::Coarse(m)), _) => self.edit_in(p, BEND, Edit::To(semis(m))),
            (Some(DataValue::Fine(f)), _) => self.edit_in(p, BEND, Edit::To(semis(f.msb()))),
            (None, Some(d)) => self.edit_addr(p, BEND, if d == Dir::Up { 1 } else { -1 }, false),
            (None, None) => false,
        };
        self.cc.room[i] -= cost;
        self.cc.ui().stepped |= moved && p == self.active_part;
    }

    /// Entry `j`, `e`, on Part `p` hears `c`.
    fn cc_bound(&mut self, p: PartId, j: usize, e: Binding, c: &Ctl, now: Ms) {
        let (a, s) = (e.target.addr(), e.span);
        if !e.target.live(self.project.part(p).sound.params.engine()) {
            return;
        }
        let (Some(spec), Some(was)) = (a.spec(), stored(&self.project, p, a)) else {
            return;
        };
        let i = p.index();
        let m = self.cc.part(i);
        // The param as the controller sees it: where this entry put it
        // while its move is still its own, else where it is.
        let ours = match spec.motion() {
            Motion::Stepped(_) => {
                let at = (was - spec.min + 0.5) as u16;
                m.pend[j] != NO_BIN || u16::from(m.bins[j]) == at
            }
            Motion::Continuous(_) => m.ramps[j].moving(),
        };
        let place = echo::param_place(spec, was);
        let rt = &mut m.rt[j];
        let seen = match rt.wrote() {
            Some(w) if ours => w,
            _ => span_of(s, place),
        };
        let act = resolve(&e, rt, c, seen);
        // Its own echo waits (spec § 9); a place it put counts as sent.
        let sent = match act {
            Some(Action::Place(to)) => echo::level(&e.input, to),
            _ => None,
        };
        self.cc.ui().echo.heard(p, j, sent, now);
        match act {
            Some(Action::Place(to)) => self.cc_place(p, j, a, s, to, now),
            Some(Action::Detents(d)) => {
                let t = &mut self.cc.part(i).turns[j];
                *t = t.saturating_add(d);
            }
            Some(Action::Wait(_)) | None => {}
        }
    }

    /// An NRPN no binding on Part `p` names (spec § 5): its address's
    /// param, if writable and read by the Part's engine, by its law, with
    /// `Jump`, through a canonical slot's ramp or bin.
    fn cc_canonical(&mut self, p: PartId, n: U14, v: DataValue, now: Ms) {
        let Some(t) = NrpnAddr::new(n).addr().and_then(nrpn::writable) else {
            return;
        };
        let s = &self.project.part(p).sound;
        if !t.live(s.params.engine()) {
            return;
        }
        let a = t.addr();
        let Some(spec) = a.spec() else {
            return;
        };
        let law = nrpn::law(a.block, spec);
        let code = |c: U7| {
            // The probe writes the block it reads: a copy's, never the Part's.
            let mut params = s.params.clone();
            let mut b = params.block_mut(a.block)?;
            nrpn::index_of_code(&mut *b, a.param, c)
        };
        let hi = nrpn::steps(spec);
        let Some(to) = place_of(law, v, hi, code) else {
            return;
        };
        let m = self.cc.part(p.index());
        // A sweep keeps its moving slot while none waits; else it waits
        // its turn as the others do.
        let k = match m.seat(t, m.wait[0].is_none()) {
            Seat::Move(k) => k,
            Seat::Wait => return m.wait_for(t, to),
            Seat::Handed => return,
        };
        m.unwait_addr(a);
        self.cc_canon_move(p, k, to, now);
    }

    /// Canonical slot `k` on Part `p` to `to`, by its target's law's place.
    fn cc_canon_move(&mut self, p: PartId, k: usize, to: Place, now: Ms) {
        let j = MAP_LEN + k;
        let Some(t) = self.cc.part(p.index()).canon[k] else {
            return;
        };
        let a = t.addr();
        let Some(spec) = a.spec() else {
            return;
        };
        match spec.motion() {
            // The index the law named, exactly: no bins to cross.
            Motion::Stepped(_) => {
                let hi = nrpn::steps(spec);
                let at = (u32::from(to.get()) * u32::from(hi) + 8191) / 16383;
                let at = at.min(u32::from(NO_BIN) - 1) as u16;
                let last = &mut self.cc.part(p.index()).bins[j];
                if u16::from(*last) != at {
                    *last = at as u8;
                    self.cc_bin(p, j, a, at);
                }
            }
            Motion::Continuous(_) => self.cc_place(p, j, a, Span::FULL, to, now),
        }
    }

    /// Part `p`'s canonical targets waiting, in the order they came, each
    /// to its own slot once at rest or a free one, as far as slots free.
    fn cc_waiting(&mut self, p: PartId, now: Ms) {
        let engine = self.project.part(p).sound.params.engine();
        let mut i = 0;
        while i < WAIT {
            let m = self.cc.part(p.index());
            let Some((t, to)) = m.wait[i] else {
                return;
            };
            if !t.live(engine) {
                m.unwait(i);
                continue;
            }
            let Seat::Move(k) = m.seat(t, false) else {
                i += 1;
                continue;
            };
            m.unwait(i);
            self.cc_canon_move(p, k, to, now);
        }
    }

    /// Slot `j`'s target `a` on Part `p` to place `to` of span `s`: a
    /// continuous one heads there; a stepped one moves to its bin through
    /// the edit path. Inlined into both callers: their shared tail call
    /// would be outlined into bank 1 (ADR 0175).
    #[inline(always)]
    fn cc_place(&mut self, p: PartId, j: usize, a: ParamAddr, s: Span, to: Place, now: Ms) {
        let (Some(spec), Some(was)) = (a.spec(), stored(&self.project, p, a)) else {
            return;
        };
        let i = p.index();
        match spec.motion() {
            Motion::Stepped(_) => {
                // One unit a step (`ParamSpec::stepped`, `choice`).
                let steps = (spec.max - spec.min) as u16 + 1;
                let at = (was - spec.min + 0.5) as u16;
                let last = &mut self.cc.part(i).bins[j];
                let cur = if *last == NO_BIN {
                    at
                } else {
                    u16::from(*last)
                };
                // A 7-bit value bins as ADR 0139's; a finer one by its MSB.
                let v = (to.get() >> 7) as u8;
                let to = bin(v, steps, s.min, s.max, cur).min(u16::from(NO_BIN) - 1);
                *last = to as u8;
                if to != cur {
                    self.cc_bin(p, j, a, to);
                }
            }
            Motion::Continuous(_) => {
                let r = &mut self.cc.part(i).ramps[j];
                // At rest, it starts from the stored value, wherever the
                // hand or the engine left it.
                if !r.moving() {
                    r.cancel(spec.normalize(was));
                }
                r.hear(to, span_place(s, to), now);
            }
        }
    }

    /// The knob writes moving `a` on Part `p` to step `to` costs (a MODEL's
    /// defaults too); `None` when a streamed source refuses it, as the
    /// hand's turn is (`turn_model`), so a key never plays into silence.
    fn cc_cost(&self, p: PartId, a: ParamAddr, to: f32) -> Option<u8> {
        let mut cost = u8::from(knob(a));
        if a == super::held::MODEL || a == super::held::DIRECTION {
            let code = to as u8;
            let s = &self.project.part(p).sound;
            if self.streams(p) && (super::held::Held { addr: a, code }).why(s, true).is_some() {
                return None;
            }
            // Its machine's or DIRECTION's knob writes too (bank 2's,
            // `walk::tape_writes_in`).
            cost += crate::bank2::ui_door(|u| u.tape_writes(a.param, code, &s.params.tape))
                .unwrap_or(0);
        }
        Some(cost)
    }

    /// Part `p`'s waiting bins, as far as this pass has room.
    fn cc_pending(&mut self, p: PartId) {
        let (i, engine) = (p.index(), self.project.part(p).sound.params.engine());
        for j in 0..SLOTS {
            let to = self.cc.part(i).pend[j];
            match self.cc_target(p, j) {
                Some(t) if to != NO_BIN && t.live(engine) => {
                    self.cc_bin(p, j, t.addr(), u16::from(to));
                }
                _ => {}
            }
        }
    }

    /// Slot `j`'s target on Part `p`: its map entry's, or a canonical one.
    #[inline(always)]
    fn cc_target(&mut self, p: PartId, j: usize) -> Option<Mappable> {
        match j.checked_sub(MAP_LEN) {
            None => self
                .project
                .part(p)
                .sound
                .map
                .entries()
                .get(j)
                .map(|e| e.target),
            Some(k) => self.cc.part(p.index()).canon[k],
        }
    }

    /// Slot `j`'s stepped target `a` on Part `p` to bin `to` through the
    /// edit path, if this pass has room for its knob writes; else it waits
    /// for the next, as a ramp does.
    fn cc_bin(&mut self, p: PartId, j: usize, a: ParamAddr, to: u16) {
        let i = p.index();
        self.cc.part(i).pend[j] = NO_BIN;
        let (Some(spec), Some(was)) = (a.spec(), stored(&self.project, p, a)) else {
            return;
        };
        let at = (was - spec.min + 0.5) as u16;
        let d = (i32::from(to) - i32::from(at)).clamp(-127, 127) as i8;
        if d == 0 {
            return;
        }
        let Some(cost) = self.cc_cost(p, a, spec.min + f32::from(to)) else {
            return;
        };
        if cost > self.cc.room[i] {
            self.cc.part(i).pend[j] = to as u8;
            return;
        }
        self.cc.room[i] -= cost;
        let moved = self.edit_addr(p, a, d, false);
        self.cc.ui().stepped |= moved && p == self.active_part;
    }

    /// Part `p`'s entries' detents heard and not yet turned: each entry's as
    /// one turn through the encoder's edit path (spec § 6.2), as far as
    /// this pass has room; the rest wait for the next.
    fn cc_turns(&mut self, p: PartId) {
        let (i, engine) = (p.index(), self.project.part(p).sound.params.engine());
        for j in 0..MAP_LEN {
            let d = self.cc.part(i).turns[j];
            if d == 0 {
                continue;
            }
            let Some(t) = self.cc_target(p, j).filter(|t| t.live(engine)) else {
                self.cc.part(i).turns[j] = 0;
                continue;
            };
            let a = t.addr();
            let d = d.clamp(-127, 127);
            let (Some(spec), Some(was)) = (a.spec(), stored(&self.project, p, a)) else {
                continue;
            };
            let to = clampf(was + f32::from(d), spec.min, spec.max);
            let Some(cost) = self.cc_cost(p, a, to) else {
                self.cc.part(i).turns[j] = 0;
                continue;
            };
            if cost > self.cc.room[i] {
                continue;
            }
            self.cc.room[i] -= cost;
            self.cc.part(i).turns[j] -= d;
            let moved = self.edit_addr(p, a, d as i8, false);
            self.cc.ui().stepped |= moved && p == self.active_part;
        }
    }

    /// A new Sound (a load, NEW, Revert, CLEAR): every slot rests, its
    /// jitter forgotten. A ramp at rest takes the stored place when it
    /// next hears (`cc_place`), so where it rests doesn't matter.
    fn cc_reset_on_load(&mut self, p: PartId) {
        let (i, loads) = (p.index(), self.project.part(p).loads());
        let m = self.cc.part(i);
        if loads != m.loads {
            m.loads = loads;
            m.reset();
        }
    }

    /// Part `p`'s moving ramps, `elapsed` ms on: every non-knob one, and
    /// round-robin as many knobs as the pass has room for. The rest keep
    /// their ramp and go first next pass.
    fn cc_step(&mut self, p: PartId, elapsed: u32) {
        let i = p.index();
        let engine = self.project.part(p).sound.params.engine();
        let mut targets = [None; SLOTS];
        let (mut due, mut own) = ([false; SLOTS], [false; SLOTS]);
        for (j, slot) in targets.iter_mut().enumerate() {
            if !self.cc.part(i).ramps[j].moving() {
                continue;
            }
            let Some(t) = self.cc_target(p, j).filter(|t| t.live(engine)) else {
                // Its engine left: a message after the return starts afresh.
                self.cc.part(i).ramps[j].cancel(0.0);
                continue;
            };
            *slot = Some(t.addr());
            if knob(t.addr()) {
                due[j] = true;
            } else {
                own[j] = true;
            }
        }
        let room = self.cc.room[i];
        let m = self.cc.part(i);
        let mut cursor = usize::from(m.cursor);
        let pick = budget(&due, &mut cursor, usize::from(room));
        m.cursor = cursor as u8;
        self.cc.room[i] -= pick.iter().filter(|&&b| b).count() as u8;
        for (j, a) in targets.into_iter().enumerate() {
            let Some(a) = a.filter(|_| pick[j] || own[j]) else {
                continue;
            };
            if let (Some(place), Some(spec)) = (self.cc.part(i).ramps[j].step(elapsed), a.spec()) {
                // The top lands on `max` itself, not its exp2 round trip.
                let v = if place >= 1.0 {
                    spec.max
                } else {
                    spec.denormalize(place)
                };
                self.edit_in(p, a, Edit::To(v));
            }
        }
    }

    /// The hand wins: a turn of `a` on Part `p` stops its ramps and drops
    /// its waiting bins, detents and canonical place, nothing heard moves it this pass, and
    /// its next message starts from the turned value, its next bin from the
    /// stored one. The door's `hand`.
    pub(super) fn hand_in(&mut self, p: PartId, a: ParamAddr) {
        self.cc.fresh();
        let i = p.index();
        let map = self.project.part(p).sound.map;
        let entry = map.entries().iter().position(|e| e.target.addr() == a);
        let m = self.cc.part(i);
        let canon = m
            .canon
            .iter()
            .position(|c| c.is_some_and(|c| c.addr() == a));
        for j in [entry, canon.map(|k| MAP_LEN + k)].into_iter().flatten() {
            m.ramps[j].cancel(0.0);
            m.bins[j] = NO_BIN;
            m.pend[j] = NO_BIN;
            m.handed |= 1 << j;
            if j < MAP_LEN {
                m.turns[j] = 0;
            }
        }
        m.unwait_addr(a);
    }
}

/// RPN 0's semitones: the MSB, clamped to `MAX_BEND`.
#[inline(always)]
fn semis(m: U7) -> f32 {
    f32::from(m.get().min(MAX_BEND))
}
