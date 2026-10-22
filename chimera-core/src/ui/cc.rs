//! Mapped CCs move the Sound (spec §§ 5-7): each Part's map heard on its
//! channel, continuous targets ramped over the gap between CCs, stepped
//! ones binned through the encoder's edit path, and only as many knob
//! writes a Part as the moves still in flight to the audio leave room
//! for, so the audio never takes more than `MOVES` at once.

use chimera_hal::Ms;

use super::UiState;
use super::nav::PartChain;
use super::part_page::Edit;
use crate::addr::{BlockRef, ParamAddr};
use crate::block::{Motion, ParamId};
use crate::dsp::knobs::MOVES;
use crate::dsp::tape_engine::machine::{defaults, direction_defaults, env_defaults};
use crate::dsp::tape_engine::{Choice, Direction, MachineIndex, TapeEngineParams, TapeMachine};
use crate::hw::MAX_PARTS;
use crate::midi_in::CcEvent;
use crate::midi_map::ramp::{Ramp, bin, budget};
use crate::midi_map::{MAP_LEN, MapEntry, MidiMap};
use crate::project::{Loads, PartId, Project};

/// No bin heard since a load or the hand: the stored value's.
const NO_BIN: u8 = u8::MAX;

/// Each Part's entries' ramps, last bins and waiting bins, its budget
/// cursor, the Sound loads they were heard on, the entries the hand
/// turned, the knob writes left this pass and the knob moves published
/// since the audio last took a publish; the hand's knob moves this pass.
/// ~1.8 KB.
pub struct CcState {
    ramps: [[Ramp; MAP_LEN]; MAX_PARTS],
    bins: [[u8; MAP_LEN]; MAX_PARTS],
    /// A stepped entry's bin that found no room: it goes first next pass.
    pend: [[u8; MAP_LEN]; MAX_PARTS],
    cursor: [u8; MAX_PARTS],
    loads: [Loads; MAX_PARTS],
    /// Entries the hand turned this pass: no CC moves them till the next.
    handed: [u16; MAX_PARTS],
    room: [u8; MAX_PARTS],
    /// Knob moves published since the audio last took a publish.
    flight: [u8; MAX_PARTS],
    last: Ms,
    hand: u8,
    /// A stepped CC moved the active Part this pass.
    stepped: bool,
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
            pend: [[NO_BIN; MAP_LEN]; MAX_PARTS],
            cursor: [0; MAX_PARTS],
            loads: [Loads::NONE; MAX_PARTS],
            handed: [0; MAX_PARTS],
            room: [0; MAX_PARTS],
            flight: [0; MAX_PARTS],
            last: Ms(0),
            hand: 0,
            stepped: false,
        }
    }

    /// A new project: no ramp or bin survives it.
    pub(super) fn clear(&mut self) {
        let (last, hand, flight) = (self.last, self.hand, self.flight);
        *self = Self {
            last,
            hand,
            flight,
            ..Self::new()
        };
    }

    /// Part `p`'s ramps at rest, bins and cursor cleared: its map changed
    /// (a learn, a load), and they are indexed by entry.
    #[inline]
    pub(super) fn reset_part(&mut self, p: PartId) {
        let i = p.index();
        self.ramps[i] = [Ramp::rest(0.0); MAP_LEN];
        self.bins[i] = [NO_BIN; MAP_LEN];
        self.pend[i] = [NO_BIN; MAP_LEN];
        self.cursor[i] = 0;
    }

    /// The knob moves the hand made this pass, at most: its turns and the
    /// reel's (`handle_input`), which the CC writes after it leave room for.
    pub(super) fn hand(&mut self, moves: u8) {
        self.hand = moves;
    }
}

/// `a`'s stored value on Part `p`.
fn stored(project: &Project, p: PartId, a: ParamAddr) -> Option<f32> {
    Some(project.read_part(p).block(a.block)?.get(a.param))
}

/// Glided by the Part's knobs: counted by the publish (`KNOBS`).
fn knob(a: ParamAddr) -> bool {
    a.spec().and_then(|s| s.motion().knob()).is_some()
}

fn is_knob(e: &MapEntry) -> bool {
    knob(e.target.addr())
}

const MODEL: ParamAddr = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODEL);

/// The knob writes a MODEL change to `m` makes besides MODEL's own: its
/// machine's defaults in DIRECTION `d` (`part_page::edit`).
pub(super) fn model_writes(m: TapeMachine, d: Direction) -> u8 {
    let k = defaults(m, d).filter(|&(id, _)| knob(ParamAddr::new(BlockRef::TapeEngine, id)));
    let env1 = BlockRef::Env(crate::dsp::modulator::EnvSlot::Env1);
    let e = env_defaults(m, d)
        .iter()
        .filter(|&&(id, _)| knob(ParamAddr::new(env1, id)));
    (k.count() + e.count()) as u8
}

/// The knob writes a DIRECTION change from `was` to `now` makes besides
/// its own (`walk::machine_edit_in`): LENGTH and FADE crossing once
/// through and a loop, and then ENV 1's for `now`.
pub(super) fn direction_writes(was: Direction, now: Direction) -> u8 {
    let writes = direction_defaults(was, now);
    let tape = |&&(id, _): &&(ParamId, f32)| knob(ParamAddr::new(BlockRef::TapeEngine, id));
    let env1 = BlockRef::Env(crate::dsp::modulator::EnvSlot::Env1);
    let env = |&&(id, _): &&(ParamId, f32)| knob(ParamAddr::new(env1, id));
    let e = if writes.is_empty() {
        0
    } else {
        env_defaults(TapeMachine::Walk, now).iter().filter(env).count()
    };
    (writes.iter().filter(tape).count() + e) as u8
}

impl UiState {
    /// Each Part's ramps `now` on, then this pass's CCs (spec §§ 5-7).
    /// After `handle_input`, before the publish: the hand's moves are in.
    /// `taken`: the audio has taken the last publish (`Writer::taken`).
    /// Card work waiting drops them (spec § 1): a write now would refuse
    /// the load or adopt its witness confirmed.
    pub fn hear_ccs(&mut self, ccs: impl Iterator<Item = CcEvent>, now: Ms, taken: bool) {
        let elapsed = now.since(self.cc.last);
        self.cc.last = now;
        let busy = self.job.is_some() || self.lib.busy() || self.src.busy();
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
        }
        for ev in ccs.filter(|_| !busy) {
            #[cfg(feature = "midi-map")]
            if self.learn_cc(ev) {
                continue;
            }
            for p in PartId::ALL {
                if self.project.part(p).mix.channel != ev.channel {
                    continue;
                }
                let (map, handed) = (self.project.part(p).sound.map, self.cc.handed[p.index()]);
                // The hand beats a CC in its pass (spec § 5).
                for (j, e) in map.targets_of(ev.cc).filter(|&(j, _)| handed >> j & 1 == 0) {
                    self.cc_hear(p, j, e, ev.value, now);
                }
            }
        }
        for (f, r) in self.cc.flight.iter_mut().zip(self.cc.room) {
            *f = (*f).max(MOVES as u8 - 1) - r;
        }
        self.cc.handed = [0; MAX_PARTS];
        // The page follows a stepped CC on the active Part as a turn's.
        if core::mem::take(&mut self.cc.stepped) && self.loc.page(&self.cx()).is_some() {
            self.after_edit(at, chain, before, self.page_def());
        }
        // A CC on a browsed Part moves its cost and meter hints.
        self.browser_dirty |= self.loc.browse().is_some() && self.project.rev() != rev;
    }

    /// Part `p`'s waiting bins, as far as this pass has room.
    fn cc_pending(&mut self, p: PartId) {
        let (i, map) = (p.index(), self.project.part(p).sound.map);
        let engine = self.project.part(p).sound.params.engine();
        for (j, e) in map.entries().iter().enumerate() {
            let to = self.cc.pend[i][j];
            if to != NO_BIN && e.target.live(engine) {
                self.cc_bin(p, j, e.target.addr(), u16::from(to));
            }
        }
    }

    /// Entry `j`'s stepped target `a` on Part `p` to bin `to` through the
    /// edit path, if this pass has room for its knob writes (a MODEL's
    /// defaults too); else it waits for the next, as a ramp does.
    fn cc_bin(&mut self, p: PartId, j: usize, a: ParamAddr, to: u16) {
        let i = p.index();
        self.cc.pend[i][j] = NO_BIN;
        let (Some(spec), Some(was)) = (a.spec(), stored(&self.project, p, a)) else {
            return;
        };
        let at = (was - spec.min + 0.5) as u16;
        let d = (i32::from(to) - i32::from(at)).clamp(-127, 127) as i8;
        if d == 0 {
            return;
        }
        let mut cost = u8::from(knob(a));
        if a == MODEL {
            let d = self.project.part(p).sound.params.tape.direction;
            cost += model_writes(
                TapeMachine::from_index(MachineIndex((spec.min + f32::from(to)) as u8)),
                d,
            );
        }
        // A DIRECTION turn's own writes, from where it is to where it lands.
        if a.block == BlockRef::TapeEngine && a.param == TapeEngineParams::MODE {
            let was = self.project.part(p).sound.params.tape.direction;
            if let Some(now) = Direction::from_value(spec.min + f32::from(to)) {
                cost += direction_writes(was, now);
            }
        }
        if cost > self.cc.room[i] {
            self.cc.pend[i][j] = to as u8;
            return;
        }
        self.cc.room[i] -= cost;
        self.cc.stepped |= self.edit_addr(p, a, d, false) && p == self.active_part;
    }

    /// A new Sound (a load, NEW, Revert, CLEAR): every ramp rests, its
    /// jitter forgotten. A ramp at rest takes the stored place when it
    /// next hears (`cc_hear`), so where it rests doesn't matter.
    fn cc_reset_on_load(&mut self, p: PartId) {
        let (i, loads) = (p.index(), self.project.part(p).loads());
        if loads != self.cc.loads[i] {
            self.cc.loads[i] = loads;
            self.cc.reset_part(p);
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
                let to = bin(v, steps, e.min, e.max, cur).min(u16::from(NO_BIN) - 1);
                *last = to as u8;
                if to != cur {
                    self.cc_bin(p, j, a, to);
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
    /// round-robin as many knobs as the pass has room for. The rest keep
    /// their ramp and go first next pass.
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
        let pick = budget(&due, &mut cursor, usize::from(self.cc.room[i]));
        self.cc.cursor[i] = cursor as u8;
        self.cc.room[i] -= pick.iter().filter(|&&b| b).count() as u8;
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

    /// The hand wins: a turn of `a` on Part `p` stops its ramp and drops
    /// its waiting bin, no CC moves it this pass, and its next CC starts
    /// from the turned value, its next bin from the stored one.
    pub(super) fn cc_hand(&mut self, p: PartId, a: ParamAddr) {
        if !cfg!(feature = "midi-map") {
            return;
        }
        let i = p.index();
        let map = &self.project.part(p).sound.map;
        if let Some(j) = map.entries().iter().position(|e| e.target.addr() == a) {
            self.cc.ramps[i][j].cancel(0.0);
            self.cc.bins[i][j] = NO_BIN;
            self.cc.pend[i][j] = NO_BIN;
            self.cc.handed[i] |= 1 << j;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::Block;

    /// A DIRECTION change's budget is the knob writes the edit makes, from
    /// every DIRECTION to every other (review of #761, m2).
    #[test]
    fn direction_writes_are_the_edits() {
        let env1 = BlockRef::Env(crate::dsp::modulator::EnvSlot::Env1);
        for was in Direction::ALL {
            for now in Direction::ALL {
                let mut s = crate::preset::Sound::init(crate::params::EngineType::Tape);
                s.params.tape.direction = was;
                // Unlike every default, so each write moves its knob.
                (s.params.tape.size, s.params.tape.drag) = (0.123, 0.456);
                let before = s.params.tape;
                s.params.tape.set(TapeEngineParams::MODE, now as u8 as f32);
                let addr = ParamAddr::new(BlockRef::TapeEngine, TapeEngineParams::MODE);
                let envs = crate::ui::walk::machine_edit_in(
                    addr,
                    f32::from(was as u8),
                    &mut s.params.tape,
                );
                let tape = crate::dsp::tape_engine::TAPE_ENGINE_SPECS
                    .iter()
                    .filter(|sp| sp.id != TapeEngineParams::MODE)
                    .filter(|sp| s.params.tape.get(sp.id) != before.get(sp.id))
                    .filter(|sp| knob(ParamAddr::new(BlockRef::TapeEngine, sp.id)))
                    .count();
                let env = envs.iter().filter(|&&(id, _)| knob(ParamAddr::new(env1, id))).count();
                assert_eq!(
                    usize::from(direction_writes(was, now)),
                    tape + env,
                    "{was:?} to {now:?}"
                );
            }
        }
    }
}
