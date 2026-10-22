//! ORBIT's musical core: pure functions over plain data. Clock, MIDI and
//! UI live in the shell. A step costs a mask lookup, and on a hit a draw,
//! a walk and a snap; a Bjorklund pass runs only when a fill is re-laid.

pub mod channel;
pub mod chords;
pub mod clock;
pub mod door;
pub mod engine;
pub mod euclid;
pub mod mutate;
pub mod records;
pub mod ring;
pub mod rng;
pub mod runner;
pub mod scene;
pub mod walk;

pub const RINGS: usize = 5;
pub const MAX_STEPS: u8 = 32;
pub const MAX_POOL: usize = 8;
pub const MAX_SEGS: usize = 8;

crate::project::bounded_index!(
    /// A ring's index, 0..5.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    RingIdx < RINGS
);

impl RingIdx {
    pub const fn get(self) -> u8 {
        self.0
    }
}
