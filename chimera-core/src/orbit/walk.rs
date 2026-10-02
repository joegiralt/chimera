//! POOL rings: 1..=8 sorted notes, walked over RANGE extra octaves.

use super::MAX_POOL;
use super::ring::Ring;
use super::rng::Rng;
use chimera_theory::Note;

/// 1..=8 notes, kept sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pool {
    notes: [Note; MAX_POOL],
    len: u8,
}

impl Pool {
    pub const fn one(note: Note) -> Pool {
        Pool {
            notes: [note; MAX_POOL],
            len: 1,
        }
    }

    pub const fn new(notes: &[Note]) -> Option<Pool> {
        if notes.is_empty() || notes.len() > MAX_POOL {
            return None;
        }
        let mut all = [notes[0]; MAX_POOL];
        let mut i = 0;
        while i < notes.len() {
            // Insertion sort: `sort_unstable` is not const.
            let mut j = i;
            while j > 0 && all[j - 1].get() > notes[i].get() {
                all[j] = all[j - 1];
                j -= 1;
            }
            all[j] = notes[i];
            i += 1;
        }
        Some(Pool {
            notes: all,
            len: notes.len() as u8,
        })
    }

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Note `i`, wrapping.
    pub const fn get(&self, i: usize) -> Note {
        self.notes[i % self.len()]
    }

    /// Note `i` (wrapping) replaced, then re-sorted.
    pub fn with(mut self, i: usize, note: Note) -> Pool {
        let n = self.len();
        self.notes[i % n] = note;
        self.notes[..n].sort_unstable();
        self
    }

    pub fn notes(&self) -> &[Note] {
        &self.notes[..self.len()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Walk {
    Up,
    Down,
    Random,
    /// Climbs then descends without repeating the ends.
    PingPong,
}

/// Where the walk plays next. Reset at Start and at a scene switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WalkState {
    pos: u8,
    falling: bool,
}

impl WalkState {
    pub const START: WalkState = WalkState {
        pos: 0,
        falling: false,
    };
}

impl Default for WalkState {
    fn default() -> Self {
        Self::START
    }
}

/// The ring's next pool note. The walk is the sorted pool repeated RANGE
/// more octaves up (octaves past 127 fold back down). Draws from `rng`
/// only in Random.
pub fn walk_next(ring: &Ring, st: &mut WalkState, rng: &mut Rng) -> Note {
    let len = ring.pool.len();
    let total = len * (ring.range.octaves() as usize + 1);
    // The pool may have shrunk under the state.
    let pos = st.pos as usize % total;
    let (play, next, falling) = match ring.walk {
        Walk::Up => (pos, (pos + 1) % total, false),
        Walk::Down => (pos, (pos + total - 1) % total, false),
        Walk::Random => (rng.below(total as u32) as usize, pos, false),
        Walk::PingPong if total == 1 => (0, 0, false),
        Walk::PingPong => {
            let falling = if pos == 0 {
                false
            } else if pos == total - 1 {
                true
            } else {
                st.falling
            };
            (pos, if falling { pos - 1 } else { pos + 1 }, falling)
        }
    };
    *st = WalkState {
        pos: next as u8,
        falling,
    };
    let mut x = ring.pool.get(play).get() as usize + 12 * (play / len);
    while x > 127 {
        x -= 12;
    }
    Note::new(x as u8).unwrap_or(Note::MAX)
}
