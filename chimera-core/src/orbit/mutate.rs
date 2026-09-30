//! MUTATE, Turing Machine style: at each loop's end a ring may drift.

use super::ring::{Harmony, NoteMode, Ring};
use super::rng::Rng;
use chimera_theory::Note;

/// One drift, applied with `Ring::apply`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mutation {
    /// Pool note `index` (POOL) or step `index`'s note (STEP) becomes
    /// `note`, stored unsnapped.
    Swap { index: usize, note: Note },
    /// The hit on step `from` moves to the free neighbour `to`.
    Nudge { from: usize, to: usize },
}

/// At a loop boundary: with probability MUTATE, one change, half a swap
/// and half a nudge. MUTATE 0 draws nothing: the lock.
pub fn mutate(ring: &Ring, h: &Harmony, rng: &mut Rng) -> Option<Mutation> {
    if ring.mutate.get() == 0 || !rng.chance(ring.mutate) {
        return None;
    }
    if rng.below(2) == 0 {
        let len = match ring.mode {
            NoteMode::Pool => ring.pool.len(),
            NoteMode::Step => ring.fill().steps().get() as usize,
        };
        let index = rng.below(len as u32) as usize;
        Some(Mutation::Swap {
            index,
            note: register_note(ring, h, rng),
        })
    } else {
        nudge(ring, rng)
    }
}

/// A random note of the key in `[C(OCTAVE), C(OCTAVE + RANGE + 1))`,
/// capped at 127.
fn register_note(ring: &Ring, h: &Harmony, rng: &mut Rng) -> Note {
    let lo = ring.octave.low().get();
    let hi = (lo as u16 + 12 * (ring.range.octaves() as u16 + 1)).min(128) as u8;
    let fits = |x: &u8| Note::new(*x).is_some_and(|n| h.key.contains(n.pc()));
    // At least an octave, so every pitch class is there.
    let k = rng.below((lo..hi).filter(fits).count() as u32) as usize;
    (lo..hi)
        .filter(fits)
        .nth(k)
        .and_then(Note::new)
        .unwrap_or(ring.octave.low())
}

/// A random hit with a free neighbour moves one step, earlier or later.
fn nudge(ring: &Ring, rng: &mut Rng) -> Option<Mutation> {
    let mask = ring.hits_mask();
    let n = mask.steps().get() as usize;
    let dest = |s: usize, later: bool| if later { (s + 1) % n } else { (s + n - 1) % n };
    let movable = |s: &usize| {
        mask.is_hit(*s) && (!mask.is_hit(dest(*s, false)) || !mask.is_hit(dest(*s, true)))
    };
    let count = (0..n).filter(movable).count();
    if count == 0 {
        return None;
    }
    let from = (0..n)
        .filter(movable)
        .nth(rng.below(count as u32) as usize)?;
    let (early, late) = (dest(from, false), dest(from, true));
    let to = match (mask.is_hit(early), mask.is_hit(late)) {
        (false, false) => {
            if rng.below(2) == 0 {
                early
            } else {
                late
            }
        }
        (false, true) => early,
        _ => late,
    };
    Some(Mutation::Nudge { from, to })
}
