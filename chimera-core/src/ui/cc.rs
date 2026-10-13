//! Mapped CCs move the Sound (spec §§ 5-7): each Part's map heard on its
//! channel, continuous targets ramped over the gap between CCs, stepped
//! ones binned through the encoder's edit path, and at most `MOVES − 1`
//! knob writes a Part a pass, so a publish never overflows.

use chimera_hal::Ms;

use super::UiState;
use super::part_page::Edit;
use crate::addr::ParamAddr;
use crate::block::Motion;
use crate::dsp::knobs::MOVES;
use crate::hw::MAX_PARTS;
use crate::midi_in::CcEvent;
use crate::midi_map::ramp::{Ramp, bin, budget};
use crate::midi_map::{MAP_LEN, MapEntry, MidiMap};
use crate::project::{Loads, PartId, Project};

/// No bin heard since a load or the hand: the stored value's.
const NO_BIN: u8 = u8::MAX;

/// Each Part's entries' ramps and last bins, its budget cursor, and the
/// Sound loads they were heard on. ~1.6 KB.
pub struct CcState {
    ramps: [[Ramp; MAP_LEN]; MAX_PARTS],
    bins: [[u8; MAP_LEN]; MAX_PARTS],
    cursor: [u8; MAX_PARTS],
    loads: [Loads; MAX_PARTS],
    last: Ms,
}

impl Default for CcState {
    fn default() -> Self {
        Self::new()
    }
}

impl CcState {
    pub const fn new() -> Self {
        Self {
            ramps: [[Ramp::rest(0.0); MAP_LEN]; MAX_PARTS],
            bins: [[NO_BIN; MAP_LEN]; MAX_PARTS],
            cursor: [0; MAX_PARTS],
            loads: [Loads::NONE; MAX_PARTS],
            last: Ms(0),
        }
    }
}

/// `a`'s stored value on Part `p`.
fn stored(project: &Project, p: PartId, a: ParamAddr) -> Option<f32> {
    Some(project.read_part(p).block(a.block)?.get(a.param))
}

/// Glided by the Part's knobs: counted by the publish (`KNOBS`).
fn is_knob(e: &MapEntry) -> bool {
    e.target
        .addr()
        .spec()
        .and_then(|s| s.motion().knob())
        .is_some()
}

impl UiState {
    /// Each Part's ramps `now` on, then this pass's CCs (spec §§ 5-7).
    pub fn hear_ccs(&mut self, ccs: impl Iterator<Item = CcEvent>, now: Ms) {
        let elapsed = now.since(self.cc.last);
        self.cc.last = now;
        // Ramps first: one heard now starts now, not `elapsed` ago.
        for p in PartId::ALL {
            self.cc_reset_on_load(p);
            if elapsed > 0 {
                self.cc_step(p, elapsed);
            }
        }
        for ev in ccs {
            for p in PartId::ALL {
                if self.project.part(p).mix.channel != ev.channel {
                    continue;
                }
                let map = self.project.part(p).sound.map;
                for (j, e) in map.targets_of(ev.cc) {
                    self.cc_hear(p, j, e, ev.value, now);
                }
            }
        }
    }

    /// A new Sound (a load, NEW, Revert, CLEAR): every ramp rests, its
    /// jitter forgotten. A ramp at rest takes the stored place when it
    /// next hears (`cc_hear`), so where it rests doesn't matter.
    fn cc_reset_on_load(&mut self, p: PartId) {
        let (i, loads) = (p.index(), self.project.part(p).loads());
        if loads != self.cc.loads[i] {
            self.cc.loads[i] = loads;
            self.cc.bins[i] = [NO_BIN; MAP_LEN];
            self.cc.ramps[i] = [Ramp::rest(0.0); MAP_LEN];
        }
    }

    /// Entry `j`'s CC value `v` on Part `p`: a continuous target heads for
    /// its place; a stepped one moves to its bin through the edit path.
    fn cc_hear(&mut self, p: PartId, j: usize, e: &MapEntry, v: u8, now: Ms) {
        let a = e.target.addr();
        if !e.target.live(self.project.part(p).sound.params.engine()) {
            return;
        }
        let (Some(spec), Some(was)) = (a.spec(), stored(&self.project, p, a)) else {
            return;
        };
        let i = p.index();
        match spec.motion() {
            Motion::Stepped(_) => {
                // One unit a step (`ParamSpec::stepped`, `choice`).
                let steps = (spec.max - spec.min) as u16 + 1;
                let at = (was - spec.min + 0.5) as u16;
                let last = &mut self.cc.bins[i][j];
                let cur = if *last == NO_BIN {
                    at
                } else {
                    u16::from(*last)
                };
                let to = bin(v, steps, e.min, e.max, cur);
                *last = to.min(u16::from(NO_BIN) - 1) as u8;
                let d = (i32::from(to) - i32::from(at)).clamp(-127, 127) as i8;
                if to != cur && d != 0 {
                    self.edit_addr(p, a, d, false);
                }
            }
            Motion::Continuous(_) => {
                let r = &mut self.cc.ramps[i][j];
                // At rest, it starts from the stored value, wherever the
                // hand or the engine left it.
                if !r.moving() {
                    r.cancel(spec.normalize(was));
                }
                r.hear(v, e.place(v), now);
            }
        }
    }

    /// Part `p`'s moving ramps, `elapsed` ms on: every non-knob one, and
    /// at most `MOVES − 1` knobs, round-robin, so a hand turn still fits
    /// the publish. The rest keep their ramp and go first next pass.
    fn cc_step(&mut self, p: PartId, elapsed: u32) {
        let i = p.index();
        let map: MidiMap = self.project.part(p).sound.map;
        let engine = self.project.part(p).sound.params.engine();
        let (mut due, mut own) = ([false; MAP_LEN], [false; MAP_LEN]);
        for (j, e) in map.entries().iter().enumerate() {
            let r = &mut self.cc.ramps[i][j];
            if !r.moving() {
                continue;
            }
            if !e.target.live(engine) {
                // Its engine left: a CC after the return starts afresh.
                r.cancel(0.0);
                continue;
            }
            if is_knob(e) {
                due[j] = true;
            } else {
                own[j] = true;
            }
        }
        let mut cursor = usize::from(self.cc.cursor[i]);
        let pick = budget(&due, &mut cursor, MOVES - 1);
        self.cc.cursor[i] = cursor as u8;
        for (j, e) in map.entries().iter().enumerate() {
            if !(pick[j] || own[j]) {
                continue;
            }
            let a = e.target.addr();
            if let (Some(place), Some(spec)) = (self.cc.ramps[i][j].step(elapsed), a.spec()) {
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

    /// The hand wins: a turn of `a` on Part `p` stops its ramp (its
    /// next CC starts from the turned value) and its next bin starts from
    /// the stored one.
    pub(super) fn cc_hand(&mut self, p: PartId, a: ParamAddr) {
        let i = p.index();
        let map = &self.project.part(p).sound.map;
        if let Some(j) = map.entries().iter().position(|e| e.target.addr() == a) {
            self.cc.ramps[i][j].cancel(0.0);
            self.cc.bins[i][j] = NO_BIN;
        }
    }
}
