//! The sympathetic slot pool's allocator (exclusive-state spec § 4.5):
//! pure bookkeeping over `SYM_SLOTS` slots, no DSP.
//!
//! The owner's rule (2026-09-29): nothing is stolen for the pool. A
//! Sympathetic note gets a slot only if one is free when it starts;
//! otherwise it plays bare, its main string alone, and never gains one
//! mid-note. A slot comes back when its voice gives the lease back.

use crate::voice_alloc::VoiceIdx;

/// Sympathetic notes sounding at once (Rings' `kMaxPolyphony`).
pub const SYM_SLOTS: usize = 4;

/// A pool slot's index: always `< SYM_SLOTS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymSlot(u8);

impl SymSlot {
    pub const ALL: [SymSlot; SYM_SLOTS] = {
        let mut all = [SymSlot(0); SYM_SLOTS];
        let mut i = 0;
        while i < SYM_SLOTS {
            all[i] = SymSlot(i as u8);
            i += 1;
        }
        all
    };

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// The right to one slot. Only [`SymAlloc::lend`] makes one, from a
/// slot promised to the voice, and only [`SymAlloc::give_back`] takes it,
/// so a slot has at most one.
///
/// It can't be copied:
///
/// ```compile_fail,E0599
/// use chimera_core::{sym_alloc::SymAlloc, voice_alloc::VoiceIdx};
/// let mut alloc = SymAlloc::new();
/// alloc.place(VoiceIdx::ALL[0]);
/// let lease = alloc.lend(VoiceIdx::ALL[0]).unwrap();
/// let b = lease.clone();
/// ```
///
/// used once given back:
///
/// ```compile_fail,E0382
/// use chimera_core::{sym_alloc::SymAlloc, voice_alloc::VoiceIdx};
/// let mut alloc = SymAlloc::new();
/// alloc.place(VoiceIdx::ALL[0]);
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
/// alloc.place(VoiceIdx::ALL[0]);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Free,
    /// Reserved for a note that hasn't started: it waits on a fade or on
    /// the clear budget.
    Promised(VoiceIdx),
    /// A `Lease` is out.
    Lent(VoiceIdx),
}

impl State {
    fn voice(self) -> Option<VoiceIdx> {
        match self {
            State::Free => None,
            State::Promised(v) | State::Lent(v) => Some(v),
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

    /// A Sympathetic note starts on `voice`: it keeps a slot it holds or
    /// is promised, else a free one is promised to it. With none free it
    /// plays bare. Whether it will ring.
    pub fn place(&mut self, voice: VoiceIdx) -> bool {
        if self.slot_of(voice).is_some() {
            return true;
        }
        let Some(s) = self.first_free() else {
            return false;
        };
        self.slots[s] = State::Promised(voice);
        true
    }

    /// `voice`'s promised slot: `None` if it has none (it plays bare) or
    /// already holds its lease.
    #[must_use = "a dropped Lease leaks its slot; give it back"]
    pub fn lend(&mut self, voice: VoiceIdx) -> Option<Lease> {
        let s = self.promise_of(voice)?;
        self.slots[s.index()] = State::Lent(voice);
        Some(Lease(s))
    }

    /// The slot `lend(voice)` would lend, lending nothing.
    pub fn promise_of(&self, voice: VoiceIdx) -> Option<SymSlot> {
        let s = self
            .slots
            .iter()
            .position(|&s| s == State::Promised(voice))?;
        Some(SymSlot(s as u8))
    }

    pub fn give_back(&mut self, lease: Lease) {
        let s = &mut self.slots[lease.0.index()];
        debug_assert!(matches!(s, State::Lent(_)), "a lease for a slot not lent");
        *s = State::Free;
    }

    /// `voice` wants no slot now: its promise, if any, is freed. Its lease,
    /// if it holds one, is untouched: only `give_back` ends that.
    pub fn cancel(&mut self, voice: VoiceIdx) {
        if let Some(s) = self.promise_of(voice) {
            self.slots[s.index()] = State::Free;
        }
    }

    /// Some slot is promised to `voice`.
    pub fn promised(&self, voice: VoiceIdx) -> bool {
        self.promise_of(voice).is_some()
    }

    /// The voice a slot is promised or lent to.
    pub fn holder(&self, slot: SymSlot) -> Option<VoiceIdx> {
        self.slots[slot.index()].voice()
    }

    pub fn free(&self) -> usize {
        self.count(|s| s == State::Free)
    }

    pub fn lent(&self) -> usize {
        self.count(|s| matches!(s, State::Lent(_)))
    }

    fn count(&self, f: impl Fn(State) -> bool) -> usize {
        self.slots.iter().filter(|&&s| f(s)).count()
    }

    /// The slot promised or lent to `v`.
    fn slot_of(&self, v: VoiceIdx) -> Option<usize> {
        self.slots.iter().position(|s| s.voice() == Some(v))
    }

    fn first_free(&self) -> Option<usize> {
        self.slots.iter().position(|&s| s == State::Free)
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

    /// `voices` placed and lent, in order.
    fn hold(a: &mut SymAlloc, voices: &[usize]) -> Vec<Lease> {
        voices
            .iter()
            .map(|&i| {
                assert!(a.place(v(i)));
                a.lend(v(i)).expect("promised")
            })
            .collect()
    }

    #[test]
    fn a_free_slot_is_promised_to_the_note() {
        let mut a = SymAlloc::new();
        assert!(a.place(v(3)));
        assert!(a.promised(v(3)));
        assert_eq!(a.free(), SYM_SLOTS - 1);
    }

    #[test]
    fn a_retrigger_keeps_its_slot() {
        let mut a = SymAlloc::new();
        let ls = hold(&mut a, &[0]);
        assert!(a.place(v(0)));
        assert_eq!((a.free(), a.lent()), (SYM_SLOTS - 1, 1));
        assert!(a.lend(v(0)).is_none(), "no second lease");
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    /// With every slot held, a fifth note plays bare: nothing moves.
    #[test]
    fn the_fifth_plays_bare() {
        let mut a = SymAlloc::new();
        let ls = hold(&mut a, &[0, 1, 2, 3]);
        assert!(!a.place(v(4)));
        assert!(a.lend(v(4)).is_none());
        for (s, i) in SymSlot::ALL.into_iter().zip(0..) {
            assert_eq!(a.holder(s), Some(v(i)), "slot {i} kept");
        }
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    /// A slot given back is free for the next note placed, not for one
    /// already playing bare (it isn't placed again).
    #[test]
    fn a_slot_given_back_goes_to_the_next_placed() {
        let mut a = SymAlloc::new();
        let mut ls = hold(&mut a, &[0, 1, 2, 3]);
        assert!(!a.place(v(4)));
        a.give_back(ls.remove(0));
        assert_eq!(a.free(), 1);
        assert!(!a.promised(v(4)), "the bare note gains nothing");
        assert!(a.place(v(5)));
        assert!(a.promised(v(5)));
        ls.extend(a.lend(v(5)));
        ls.into_iter().for_each(|l| a.give_back(l));
    }

    #[test]
    fn lend_takes_only_a_promise() {
        let mut a = SymAlloc::new();
        assert!(a.lend(v(0)).is_none(), "unplaced: bare");
        assert!(a.place(v(0)));
        let l = a.lend(v(0)).expect("promised");
        assert!(!a.promised(v(0)));
        a.give_back(l);
        assert_eq!(a.free(), SYM_SLOTS);
    }

    #[test]
    fn cancel_frees_a_promise_never_a_lease() {
        let mut a = SymAlloc::new();
        let ls = hold(&mut a, &[0]);
        assert!(a.place(v(1)));
        a.cancel(v(1));
        a.cancel(v(0));
        assert_eq!((a.free(), a.lent()), (SYM_SLOTS - 1, 1));
        ls.into_iter().for_each(|l| a.give_back(l));
        assert_eq!(a.free(), SYM_SLOTS);
    }

    /// A seeded walk of `place`, `lend`, `give_back` and `cancel`: no voice
    /// is ever named by two slots, no slot is lent twice, a placed note
    /// rings exactly when a slot was free or already its own, and every
    /// lease comes home.
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
        let (mut bare, mut lent) = (0, 0);
        for step in 0..100_000 {
            let voice = v(next() as usize % MAX_VOICES);
            match next() % 4 {
                0 => {
                    let could = a.slot_of(voice).is_some() || a.free() > 0;
                    let rings = a.place(voice);
                    assert_eq!(rings, could, "step {step}");
                    bare += !rings as u32;
                }
                1 => {
                    let promised = a.promised(voice);
                    let l = a.lend(voice);
                    assert_eq!(l.is_some(), promised, "step {step}");
                    lent += l.is_some() as u32;
                    held.extend(l);
                }
                2 if !held.is_empty() => {
                    let lease = held.swap_remove(next() as usize % held.len());
                    a.give_back(lease);
                }
                2 => {}
                _ => {
                    a.cancel(voice);
                    assert!(!a.promised(voice), "step {step}");
                }
            }
            for u in VoiceIdx::ALL {
                let named = a.slots.iter().filter(|s| s.voice() == Some(u)).count();
                assert!(named <= 1, "step {step}: {u:?} named by {named} slots");
            }
            let mut slots: Vec<usize> = held.iter().map(|l| l.slot().index()).collect();
            slots.sort_unstable();
            slots.dedup();
            assert_eq!(slots.len(), held.len(), "step {step}: a slot lent twice");
            assert_eq!(a.lent(), held.len(), "step {step}");
        }
        held.into_iter().for_each(|l| a.give_back(l));
        assert_eq!(a.lent(), 0);
        assert!(bare > 0 && lent > 0, "{bare} {lent}");
    }
}
