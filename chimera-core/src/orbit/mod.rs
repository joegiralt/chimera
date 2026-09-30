//! ORBIT's musical core: pure functions over plain data. Clock, MIDI and
//! UI live in the shell. A step costs a mask lookup, and on a hit a draw,
//! a walk and a snap; a Bjorklund pass runs only when a fill is re-laid.

pub mod euclid;
pub mod mutate;
pub mod ring;
pub mod rng;
pub mod walk;

use core::mem::size_of;

pub const RINGS: usize = 5;
pub const MAX_STEPS: u8 = 32;
pub const MAX_POOL: usize = 8;

/// A ring's index, 0..5.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RingIdx(u8);

impl RingIdx {
    pub const ALL: [RingIdx; RINGS] = [RingIdx(0), RingIdx(1), RingIdx(2), RingIdx(3), RingIdx(4)];

    pub const fn new(i: u8) -> Option<RingIdx> {
        if (i as usize) < RINGS {
            Some(RingIdx(i))
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Bytes of ORBIT's pure state: the rings, their walks and dice, and the
/// harmony.
pub const STATE_BYTES: usize = size_of::<[ring::Ring; RINGS]>()
    + size_of::<[walk::WalkState; RINGS]>()
    + size_of::<[rng::Rng; RINGS]>()
    + size_of::<ring::Harmony>();

const _: () = assert!(STATE_BYTES < 1024);
