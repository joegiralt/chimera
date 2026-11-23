// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! MIDI CONFIG › ECHO › PARAM OUT (ADR 0258): each edit the hand makes
//! goes out as its NRPN (ADR 0216's address and law) on the Part's
//! channel. Pushed by the hand's own call (`hand_in`), so a value MIDI or
//! a load moved is never sent; coalesced, each param's latest value once a
//! pass while DIN's ring has room.
//!
//! Bank 1's (ADR 0251: bank 2 is under its floor), called only from bank
//! 2's `ui::midi`; each entry `inline(never)` so it stays here, and it
//! calls nothing of bank 2's: the NRPN bytes are written here, not by
//! `chimera_midi::out`.

use chimera_midi::ctl::DataValue;
use chimera_midi::law::ValueLaw;
use chimera_midi::value::{U7, U14};

use crate::addr::{BlockRef, ParamAddr};
use crate::hw::MAX_PARTS;
use crate::midi_map::Mappable;
use crate::nrpn::{self, NrpnAddr};
use crate::out_ring::{OutPorts, OutRing};
use crate::project::{Loads, PartId, Project};

/// Params turned and not yet sent: the six encoders' and a few more.
const SLOTS: usize = 8;

/// An NRPN with its fine value, status included.
const MSG_MAX: usize = 9;

/// A param the hand turned: its Part and target, the value last sent
/// (`NONE`: not yet), whether it waits to go, and whether USB is owed it.
#[derive(Clone, Copy)]
struct Turned {
    part: PartId,
    at: Mappable,
    sent: u16,
    due: bool,
    owed: bool,
}

/// Past any 14-bit value: nothing sent.
const NONE: u16 = u16::MAX;

#[derive(Clone, Copy)]
pub struct ParamOut {
    /// Oldest turn first.
    slots: [Option<Turned>; SLOTS],
    /// The loads each Part was on when its turns were noted: a load since
    /// drops them, so a turn never sends the Sound that replaced it.
    loads: [Loads; MAX_PARTS],
}

impl ParamOut {
    pub const REST: Self = Self {
        slots: [None; SLOTS],
        loads: [Loads::NONE; MAX_PARTS],
    };

    /// The hand turned `a` on Part `p`: its NRPN goes at the next pass, if
    /// it is writable (`W` on the chart), so it plays back. It becomes the
    /// newest, in a free slot, else the oldest sent, else the oldest.
    #[inline(never)]
    pub fn turned(&mut self, p: PartId, a: ParamAddr) {
        let Some(at) = Mappable::new(a).filter(|_| NrpnAddr::of(a).is_some()) else {
            return;
        };
        let same = |t: &Turned| t.part == p && t.at == at;
        let i = self
            .slots
            .iter()
            .position(|s| s.is_some_and(|t| same(&t)))
            .or_else(|| self.slots.iter().position(|s| s.is_none_or(|t| !t.due)))
            .unwrap_or(0);
        let was = self.slots[i].filter(same);
        for j in i..SLOTS - 1 {
            self.slots.swap(j, j + 1);
        }
        self.slots[SLOTS - 1] = Some(Turned {
            part: p,
            at,
            sent: was.map_or(NONE, |t| t.sent),
            due: true,
            owed: was.is_some_and(|t| t.owed),
        });
    }

    /// PARAM OUT is off: nothing waits.
    pub fn clear(&mut self) {
        self.slots = [None; SLOTS];
    }

    /// One pass: what USB is owed, into its ring alone once its poll has
    /// dropped what it held; then each waiting param's value now, on its
    /// Part's channel, while DIN's ring has room; a value already sent is
    /// skipped.
    #[inline(never)]
    pub fn pump(&mut self, project: &Project, out: &OutPorts) {
        for (l, p) in self.loads.iter_mut().zip(PartId::ALL) {
            let now = project.part(p).loads();
            if *l != now {
                *l = now;
                self.slots
                    .iter_mut()
                    .filter(|s| s.is_some_and(|t| t.part == p))
                    .for_each(|s| *s = None);
            }
        }
        if let Some(u) = out.usb.filter(|u| u.is_empty() && !u.losing()) {
            for t in self.slots.iter_mut().flatten().filter(|t| t.owed) {
                let mut b = [0u8; MSG_MAX];
                if let Some((st, n, d)) = now(project, t) {
                    let len = message(&mut b, true, st, n, d);
                    if !u.push_msg(&b[..len]) {
                        break;
                    }
                }
                t.owed = false;
            }
        }
        let mut tee = out.tee();
        let mut status = 0;
        for s in self.slots.iter_mut() {
            let Some(t) = s.as_mut().filter(|t| t.due) else {
                continue;
            };
            let Some((st, n, d)) = now(project, t) else {
                *s = None;
                continue;
            };
            let key = match d {
                DataValue::Coarse(m) => u16::from(m.get()),
                DataValue::Fine(f) => f.get(),
            };
            if key == t.sent {
                t.due = false;
                continue;
            }
            let mut b = [0u8; MSG_MAX];
            let len = message(&mut b, st != status, st, n, d);
            if !tee.send(&b[..len]) {
                break;
            }
            status = st;
            t.sent = key;
            t.due = false;
        }
        // A loss, this pass's or one not yet taken: USB's poll drops all
        // it holds, so USB is owed every turn's value.
        if tee.lost() || out.usb.is_some_and(OutRing::losing) {
            self.slots.iter_mut().flatten().for_each(|t| t.owed = true);
        }
    }
}

/// `t`'s message now: its status, NRPN and data.
#[inline(never)]
fn now(project: &Project, t: &Turned) -> Option<(u8, U14, DataValue)> {
    let a = t.at.addr();
    let (n, d) = (NrpnAddr::of(a)?, data(project, t.part, a)?);
    Some((0xB0 | project.part(t.part).mix.channel.get(), n.get(), d))
}

/// CC 99, 98, 6 and, for `Fine`, 38 on status `st`, the status written
/// only when `lead` (running status within a pass). Its length.
fn message(b: &mut [u8; MSG_MAX], lead: bool, st: u8, n: U14, d: DataValue) -> usize {
    let mut i = 0;
    if lead {
        b[0] = st;
        i = 1;
    }
    let (v, fine) = match d {
        DataValue::Coarse(m) => (m, None),
        DataValue::Fine(f) => (f.msb(), Some(f.lsb())),
    };
    let ccs = [(99, n.msb()), (98, n.lsb()), (6, v)];
    for (c, x) in ccs.into_iter().chain(fine.map(|f| (38, f))) {
        b[i] = c;
        b[i + 1] = x.get();
        i += 2;
    }
    i
}

/// `a`'s value on Part `p` as its NRPN carries it (ADR 0216), the in
/// law's inverse: `Place14` its place, 14-bit; `Index7` its steps from
/// the minimum in the MSB; `Index14` the same, 14-bit; `Code7` its frozen
/// code in the MSB.
#[inline(never)]
pub fn data(project: &Project, p: PartId, a: ParamAddr) -> Option<DataValue> {
    let spec = a.spec()?;
    let v = super::midi::stored(project, p, a)?;
    // In the spec's steps (FDBK's tenths, ADR 0299), as `nrpn::steps` counts.
    let index = || libm::roundf((v - spec.min) / spec.step).max(0.0) as u16;
    Some(match nrpn::law(a.block, spec) {
        ValueLaw::Place14 => {
            let x = libm::roundf(crate::dsp::clampf(spec.normalize(v), 0.0, 1.0) * 16383.0);
            DataValue::Fine(U14::new(x as u16)?)
        }
        ValueLaw::Index7 => DataValue::Coarse(U7::new(index().min(127) as u8)?),
        ValueLaw::Index14 => DataValue::Fine(U14::new(index().min(16383))?),
        ValueLaw::Code7 => {
            let c = match a.block {
                BlockRef::OrbitRing(r) => {
                    let ring = project.orbit().live().rings.get(usize::from(r.get()))?;
                    crate::block::Block::enum_code(ring, a.param)?
                }
                b => project.read_part(p).block(b)?.enum_code(a.param)?,
            };
            DataValue::Coarse(U7::new(c)?)
        }
    })
}
