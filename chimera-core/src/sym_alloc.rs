//! The sympathetic slot pool's allocator (exclusive-state spec § 4.5):
//! pure bookkeeping over `SYM_SLOTS` slots, no DSP.
//!
//! The Rings rule (`rings/dsp/part.cc`, the idea only): a new note always
//! sounds, and with every slot held the oldest yields. A slot never moves
//! between voices: a steal plays the new note on the slot's own voice.
//!
//! Ages are the `Allocator`'s note ages, lower is older; the pool keeps no
//! clock of its own.

use crate::voice_alloc::VoiceIdx;

/// Sympathetic notes sounding at once (Rings' `kMaxPolyphony`).
pub const SYM_SLOTS: usize = 4;

/// A pool slot's index: always `< SYM_SLOTS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymSlot(u8);

impl SymSlot {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// The right to one slot. Only [`SymAlloc::lend`] makes one and only
/// [`SymAlloc::give_back`] takes it, so a slot has at most one.
///
/// It can't be copied:
///
/// ```compile_fail,E0599
/// use chimera_core::{sym_alloc::SymAlloc, voice_alloc::VoiceIdx};
/// let mut alloc = SymAlloc::new();
/// let lease = alloc.lend(VoiceIdx::ALL[0]).unwrap();
/// let b = lease.clone();
/// ```
///
/// used once given back:
///
/// ```compile_fail,E0382
/// use chimera_core::{sym_alloc::SymAlloc, voice_alloc::VoiceIdx};
/// let mut alloc = SymAlloc::new();
/// let lease = alloc.lend(VoiceIdx::ALL[0]).unwrap();
/// alloc.give_back(lease);
/// let _ = lease.slot();
/// ```
///
/// or made outside the pool:
///
/// ```compile_fail,E0423
/// use chimera_core::{sym_alloc::{Lease, SymAlloc}, voice_alloc::VoiceIdx};
/// let mut alloc = SymAlloc::new();
/// let lease = alloc.lend(VoiceIdx::ALL[0]).unwrap();
/// let slot = lease.slot();
/// let _ = Lease(slot);
/// ```
#[must_use = "a dropped Lease leaks its slot; give it back"]
#[derive(Debug)]
pub struct Lease(SymSlot);

impl Lease {
    pub fn slot(&self) -> SymSlot {
        self.0
    }
}

/// Where a Sympathetic note plays. `drops` is a note that was waiting on
/// the slot: the newer note wins, and the `Instrument` drops that one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// On the pick, which holds or is promised a slot.
    On {
        voice: VoiceIdx,
        drops: Option<VoiceIdx>,
    },
    /// On this voice, whose slot was the oldest: it fades, then plays.
    Steal {
        voice: VoiceIdx,
        drops: Option<VoiceIdx>,
    },
    /// The CPU budget gave no voice. The pool itself never refuses.
    Refused,
}

/// A held note's fate when its Part switches to Sympathetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restart {
    /// It restarts; `evict` gave up its slot for it, and `drops` was
    /// waiting on a slot already fading, which the newer note takes.
    Claimed {
        evict: Option<VoiceIdx>,
        drops: Option<VoiceIdx>,
    },
    /// Four newer notes hold the slots: it fades and stays silent.
    Silent,
}

/// Where a lent slot goes when its lease comes back.
#[derive(Clone, Copy, Debug)]
enum Then {
    Free,
    To(VoiceIdx),
}

#[derive(Clone, Copy, Debug)]
enum State {
    Free,
    /// Reserved for a note that hasn't started.
    Promised {
        voice: VoiceIdx,
        age: u32,
    },
    /// A `Lease` is out.
    Lent {
        voice: VoiceIdx,
        age: u32,
        then: Then,
    },
}

impl State {
    /// Promised or lent to `v`, or lent elsewhere and bound for it.
    fn names(&self, v: VoiceIdx) -> bool {
        match *self {
            State::Free => false,
            State::Promised { voice, .. } => voice == v,
            State::Lent { voice, then, .. } => voice == v || matches!(then, Then::To(w) if w == v),
        }
    }
}

pub struct SymAlloc {
    slots: [State; SYM_SLOTS],
}

impl Default for SymAlloc {
    fn default() -> Self {
        Self::new()
    }
}

impl SymAlloc {
    pub const fn new() -> Self {
        Self {
            slots: [State::Free; SYM_SLOTS],
        }
    }

    /// A note-on for `pick` (`None`: the budget refused it) at `age`.
    pub fn place(&mut self, pick: Option<VoiceIdx>, age: u32) -> Place {
        if let Some(p) = pick
            && let Some(s) = self.slot_of(p)
        {
            let drops = self.waiter(s).filter(|&w| w != p);
            self.retarget(s, age, p);
            return Place::On { voice: p, drops };
        }
        match (self.first_free(), pick) {
            (Some(s), Some(p)) => {
                self.slots[s] = State::Promised { voice: p, age };
                Place::On {
                    voice: p,
                    drops: None,
                }
            }
            (Some(_), None) => Place::Refused,
            (None, _) => {
                let s = self.oldest().expect("four slots, none free");
                let u = self.voice_at(s).expect("a held slot has a voice");
                let drops = self.waiter(s);
                self.retarget(s, age, u);
                Place::Steal { voice: u, drops }
            }
        }
    }

    /// A held note at `age` whose Part just became Sympathetic. Call it
    /// newest first, so the last four played claim the slots.
    pub fn restart(&mut self, voice: VoiceIdx, age: u32) -> Restart {
        if self.slot_of(voice).is_some() {
            return Restart::Claimed {
                evict: None,
                drops: None,
            };
        }
        if let Some(s) = self.first_free() {
            self.slots[s] = State::Promised { voice, age };
            return Restart::Claimed {
                evict: None,
                drops: None,
            };
        }
        let Some(s) = self.oldest().filter(|&s| self.age_at(s) < age) else {
            return Restart::Silent;
        };
        // A slot already fading for another waiter isn't evicted again:
        // that waiter loses it instead.
        let drops = self.waiter(s);
        let evict = self.voice_at(s).filter(|_| drops.is_none());
        self.slots[s] = match self.slots[s] {
            State::Lent { voice: u, .. } => State::Lent {
                voice: u,
                age,
                then: Then::To(voice),
            },
            _ => State::Promised { voice, age },
        };
        Restart::Claimed { evict, drops }
    }

    /// `voice`'s promised slot, else a free one. `None` if it already holds
    /// one, waits on another's fade, or the pool is full.
    #[must_use = "a dropped Lease leaks its slot; give it back"]
    pub fn lend(&mut self, voice: VoiceIdx) -> Option<Lease> {
        let s = match self.slot_of(voice) {
            Some(s) if matches!(self.slots[s], State::Promised { .. }) => s,
            Some(_) => return None,
            None => self.first_free()?,
        };
        // A free slot taken unpromised gets age 0 on purpose: it is the
        // first stolen, and it can never block a restart.
        self.slots[s] = State::Lent {
            voice,
            age: self.age_at(s),
            then: Then::Free,
        };
        Some(Lease(SymSlot(s as u8)))
    }

    pub fn give_back(&mut self, lease: Lease) {
        let s = lease.0.index();
        self.slots[s] = match self.slots[s] {
            State::Lent {
                age,
                then: Then::To(w),
                ..
            } => State::Promised { voice: w, age },
            State::Lent { .. } => State::Free,
            other => {
                debug_assert!(false, "a lease for a slot not lent");
                other
            }
        };
    }

    /// `voice` has a slot coming once another voice's fade ends.
    pub fn awaits(&self, voice: VoiceIdx) -> bool {
        (0..SYM_SLOTS).any(|s| self.waiter(s) == Some(voice))
    }

    /// Frees `voice`'s promise, if it has one.
    pub fn forfeit(&mut self, voice: VoiceIdx) {
        if let Some(s) = self.slot_of(voice)
            && matches!(self.slots[s], State::Promised { .. })
        {
            self.slots[s] = State::Free;
        }
    }

    /// The voice a slot is promised or lent to.
    pub fn holder(&self, slot: SymSlot) -> Option<VoiceIdx> {
        self.voice_at(slot.index())
    }

    pub fn free(&self) -> usize {
        self.count(|s| matches!(s, State::Free))
    }

    pub fn lent(&self) -> usize {
        self.count(|s| matches!(s, State::Lent { .. }))
    }

    fn count(&self, f: impl Fn(&State) -> bool) -> usize {
        self.slots.iter().filter(|s| f(s)).count()
    }

    /// The slot promised or lent to `v`, or lent elsewhere and bound for it.
    fn slot_of(&self, v: VoiceIdx) -> Option<usize> {
        self.slots.iter().position(|s| s.names(v))
    }

    /// The voice waiting for `s`'s holder to fade: its `To`, if not the holder.
    fn waiter(&self, s: usize) -> Option<VoiceIdx> {
        match self.slots[s] {
            State::Lent {
                voice,
                then: Then::To(w),
                ..
            } if w != voice => Some(w),
            _ => None,
        }
    }

    fn first_free(&self) -> Option<usize> {
        self.slots.iter().position(|s| matches!(s, State::Free))
    }

    /// The held slot with the lowest age; the lower index wins a tie.
    fn oldest(&self) -> Option<usize> {
        (0..SYM_SLOTS)
            .filter(|&s| !matches!(self.slots[s], State::Free))
            .min_by_key(|&s| self.age_at(s))
    }

    /// 0 for a free slot.
    fn age_at(&self, s: usize) -> u32 {
        match self.slots[s] {
            State::Free => 0,
            State::Promised { age, .. } | State::Lent { age, .. } => age,
        }
    }

    fn voice_at(&self, s: usize) -> Option<VoiceIdx> {
        match self.slots[s] {
            State::Free => None,
            State::Promised { voice, .. } | State::Lent { voice, .. } => Some(voice),
        }
    }

    /// A newer note for `s`: it takes `age`, and a lent slot goes to `to`.
    fn retarget(&mut self, s: usize, age: u32, to: VoiceIdx) {
        self.slots[s] = match self.slots[s] {
            State::Lent { voice, .. } => State::Lent {
                voice,
                age,
                then: Then::To(to),
            },
            State::Promised { voice, .. } => State::Promised { voice, age },
            State::Free => State::Free,
        };
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use super::*;
    use crate::hw::MAX_VOICES;

    fn v(i: usize) -> VoiceIdx {
        VoiceIdx::ALL[i]
    }

    fn slot(i: usize) -> SymSlot {
        SymSlot(i as u8)
    }

    fn on(i: usize) -> Place {
        Place::On {
            voice: v(i),
            drops: None,
        }
    }

    fn steal(i: usize) -> Place {
        Place::Steal {
            voice: v(i),
            drops: None,
        }
    }

    fn claimed(evict: Option<usize>) -> Restart {
        Restart::Claimed {
            evict: evict.map(v),
            drops: None,
        }
    }

    /// Places and lends `v(i)` for each `i` in `voices`, at ages 1, 2, 3, …
    fn hold(a: &mut SymAlloc, voices: &[usize]) -> Vec<Lease> {
        voices
            .iter()
            .zip(1..)
            .map(|(&i, age)| {
                assert_eq!(a.place(Some(v(i)), age), on(i));
                a.lend(v(i)).expect("a promised slot lends")
            })
            .collect()
    }

    #[test]
    fn a_free_slot_is_promised_to_the_pick() {
        let mut a = SymAlloc::new();
        assert_eq!(a.place(Some(v(0)), 1), on(0));
        assert_eq!((a.free(), a.lent()), (3, 0));
        let l = a.lend(v(0)).expect("promised");
        assert_eq!(l.slot().index(), 0);
        assert_eq!(a.lent(), 1);
        a.give_back(l);
    }

    #[test]
    fn a_retrigger_keeps_its_slot() {
        let mut a = SymAlloc::new();
        let ls = hold(&mut a, &[0]);
        assert_eq!(a.place(Some(v(0)), 2), on(0));
        assert_eq!(a.free(), 3);
        assert_eq!(a.holder(slot(0)), Some(v(0)));
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    #[test]
    fn the_fifth_steals_the_oldest() {
        let mut a = SymAlloc::new();
        let ls = hold(&mut a, &[0, 1, 2, 3]);
        for (pick, age, stolen) in [(4, 5, 0), (5, 6, 1), (6, 7, 2), (7, 8, 3), (4, 9, 0)] {
            assert_eq!(a.place(Some(v(pick)), age), steal(stolen));
            assert_eq!((a.free(), a.lent()), (0, 4));
        }
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    #[test]
    fn a_full_pool_never_refuses() {
        let mut a = SymAlloc::new();
        let mut ls = hold(&mut a, &[0, 1, 2, 3]);
        assert_eq!(a.place(None, 5), steal(0));
        a.give_back(ls.remove(3));
        assert_eq!(a.place(None, 6), Place::Refused);
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    #[test]
    fn a_mono_holder_is_stolen_when_newest_arrives() {
        // v(0) stands for a Mono Part's voice: the pool doesn't know Mono.
        let mut a = SymAlloc::new();
        let ls = hold(&mut a, &[0, 1, 2, 3]);
        assert_eq!(a.place(Some(v(4)), 5), steal(0));
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    #[test]
    fn a_stolen_slot_comes_back_promised() {
        let mut a = SymAlloc::new();
        let mut ls = hold(&mut a, &[0, 1, 2, 3]);
        assert_eq!(a.place(Some(v(4)), 5), steal(0));
        a.give_back(ls.remove(0));
        assert_eq!(a.holder(slot(0)), Some(v(0)));
        assert_eq!(a.free(), 0);
        assert!(a.lend(v(5)).is_none());
        let l0 = a.lend(v(0)).expect("its own slot, promised back");
        assert_eq!(l0.slot(), slot(0));
        a.give_back(l0);
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    #[test]
    fn last_four_played_restart_on_switch() {
        let mut a = SymAlloc::new();
        for i in (0..8).rev() {
            let want = if i >= 4 {
                claimed(None)
            } else {
                Restart::Silent
            };
            assert_eq!(a.restart(v(i), i as u32 + 1), want, "v({i})");
        }

        // v(6) and v(7): another Part's older notes, sounding.
        let mut a = SymAlloc::new();
        let mut ls = hold(&mut a, &[6, 7]);
        assert_eq!(a.restart(v(5), 10), claimed(None));
        assert_eq!(a.restart(v(4), 9), claimed(None));
        assert_eq!(a.restart(v(3), 8), claimed(Some(6)));
        assert_eq!(a.restart(v(2), 7), claimed(Some(7)));
        assert!(a.awaits(v(3)) && a.awaits(v(2)));
        assert!(a.lend(v(3)).is_none());
        // A restart never evicts a newer note.
        assert_eq!(a.restart(v(1), 3), Restart::Silent);

        ls.drain(..).for_each(|l| a.give_back(l));
        assert!(!a.awaits(v(3)) && !a.awaits(v(2)));
        for i in [3, 2, 5, 4] {
            ls.push(a.lend(v(i)).expect("claimed"));
        }
        assert_eq!(a.lent(), 4);
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    /// Four held, then `v(4)`'s restart takes `v(0)`'s slot (age 5), and
    /// the other three retrigger past it: slot 0 is the oldest, `v(0)` is
    /// fading and `v(4)` waits on it.
    fn evicted_oldest() -> (SymAlloc, Vec<Lease>) {
        let mut a = SymAlloc::new();
        let ls = hold(&mut a, &[0, 1, 2, 3]);
        assert_eq!(a.restart(v(4), 5), claimed(Some(0)));
        for (i, age) in [(1, 6), (2, 7), (3, 8)] {
            assert_eq!(a.place(Some(v(i)), age), on(i));
        }
        assert!(a.awaits(v(4)));
        (a, ls)
    }

    #[test]
    fn a_retrigger_of_an_evicted_voice_drops_its_waiter() {
        let (mut a, ls) = evicted_oldest();
        let want = Place::On {
            voice: v(0),
            drops: Some(v(4)),
        };
        assert_eq!(a.place(Some(v(0)), 9), want);
        assert!(!a.awaits(v(4)));
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    #[test]
    fn a_waiter_placed_again_keeps_its_claim() {
        let (mut a, ls) = evicted_oldest();
        assert_eq!(a.place(Some(v(4)), 9), on(4));
        assert!(a.awaits(v(4)));
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    #[test]
    fn a_steal_of_an_evicted_slot_drops_its_waiter() {
        let (mut a, ls) = evicted_oldest();
        let want = Place::Steal {
            voice: v(0),
            drops: Some(v(4)),
        };
        assert_eq!(a.place(Some(v(5)), 9), want);
        assert!(!a.awaits(v(4)));
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    #[test]
    fn a_restart_on_an_evicted_slot_drops_its_waiter() {
        // v(0) already fades: the one who loses is v(4), not v(0).
        let (mut a, ls) = evicted_oldest();
        let want = Restart::Claimed {
            evict: None,
            drops: Some(v(4)),
        };
        assert_eq!(a.restart(v(5), 9), want);
        assert!(a.awaits(v(5)) && !a.awaits(v(4)));
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    #[test]
    fn lend_takes_the_promise_first() {
        let mut a = SymAlloc::new();
        assert_eq!(a.place(Some(v(2)), 1), on(2));
        assert_eq!(a.place(Some(v(3)), 2), on(3));
        let l3 = a.lend(v(3)).expect("promised");
        let l2 = a.lend(v(2)).expect("promised");
        assert_eq!((l3.slot(), l2.slot()), (slot(1), slot(0)));
        a.give_back(l3);
        a.give_back(l2);
    }

    #[test]
    fn forfeit_frees_only_a_promise() {
        let mut a = SymAlloc::new();
        assert_eq!(a.place(Some(v(0)), 1), on(0));
        assert_eq!(a.place(Some(v(1)), 2), on(1));
        let l1 = a.lend(v(1)).expect("promised");
        a.forfeit(v(1));
        assert_eq!((a.lent(), a.free()), (1, 2));
        a.forfeit(v(0));
        assert_eq!(a.free(), 3);
        a.give_back(l1);
    }

    #[test]
    fn a_holding_voice_is_not_lent_a_second_slot() {
        let mut a = SymAlloc::new();
        let ls = hold(&mut a, &[0]);
        assert!(a.lend(v(0)).is_none());
        assert_eq!(a.place(Some(v(0)), 2), on(0));
        assert!(a.lend(v(0)).is_none());
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    #[test]
    fn sym_alloc_never_gives_a_voice_two_slots() {
        let mut rng = 0x9E37_79B9_u32;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng
        };
        let mut a = SymAlloc::new();
        let mut held: Vec<Lease> = Vec::new();
        let mut age = 0u32;
        // What the walk reached: it must reach each.
        let (mut steals, mut evicts, mut dropped) = (0, 0, 0);
        for step in 0..100_000 {
            age += 1;
            let voice = v(next() as usize % MAX_VOICES);
            let awaited = VoiceIdx::ALL.map(|u| a.awaits(u));
            let full = a.free() == 0;
            let min_age = (0..SYM_SLOTS).map(|s| a.age_at(s)).min();
            let ages = (0..SYM_SLOTS).map(|s| (a.voice_at(s), a.age_at(s)));
            let ages: Vec<_> = ages.collect();
            let mut drops = None;
            let mut returned = None;
            match next() % 5 {
                0 => {
                    let pick = (next() % 8 != 0).then_some(voice);
                    let had_slot = pick.and_then(|p| a.slot_of(p)).is_some();
                    let placed = a.place(pick, age);
                    match placed {
                        Place::Refused => assert!(pick.is_none() && !full, "step {step}"),
                        Place::On { voice: u, drops: d } => {
                            assert_eq!(Some(u), pick, "step {step}");
                            drops = d;
                        }
                        Place::Steal { voice: u, drops: d } => {
                            let (_, stolen_age) = ages
                                .iter()
                                .find(|(w, _)| *w == Some(u))
                                .expect("the stolen voice held a slot");
                            assert_eq!(Some(*stolen_age), min_age, "step {step}: not the oldest");
                            steals += 1;
                            drops = d;
                        }
                    }
                    let stole = matches!(placed, Place::Steal { .. });
                    assert_eq!(stole, full && !had_slot, "step {step}: {placed:?}");
                }
                1 => {
                    if let Restart::Claimed { evict, drops: d } = a.restart(voice, age) {
                        evicts += evict.is_some() as u32;
                        drops = d;
                    }
                }
                2 => held.extend(a.lend(voice)),
                3 if !held.is_empty() => {
                    let lease = held.swap_remove(next() as usize % held.len());
                    returned = Some(lease.slot());
                    a.give_back(lease);
                }
                3 => {}
                _ => a.forfeit(voice),
            }

            if let Some(d) = drops {
                assert!(
                    awaited[d.index()],
                    "step {step}: {d:?} dropped, never waiting"
                );
                assert_eq!(
                    a.slot_of(d),
                    None,
                    "step {step}: {d:?} dropped, still named"
                );
                dropped += 1;
            }
            for u in VoiceIdx::ALL {
                let named = a.slots.iter().filter(|s| s.names(u)).count();
                assert!(named <= 1, "step {step}: {u:?} named by {named} slots");
                if awaited[u.index()] && !a.awaits(u) {
                    let promised = returned.is_some_and(|s| {
                        matches!(a.slots[s.index()], State::Promised { voice, .. } if voice == u)
                    });
                    assert!(
                        drops == Some(u) || promised,
                        "step {step}: {u:?} lost unreported"
                    );
                }
            }
            assert_eq!(a.lent(), held.len(), "step {step}");
        }
        held.into_iter().for_each(|l| a.give_back(l));
        assert_eq!(a.lent(), 0);
        assert!(
            steals > 0 && evicts > 0 && dropped > 0,
            "{steals} {evicts} {dropped}"
        );
    }
}
