//! ORBIT's clock types.

/// A position within a bar, in 2³² ths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Phase(u32);

impl Phase {
    pub const ZERO: Phase = Phase(0);

    pub const fn new(p: u32) -> Phase {
        Phase(p)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}
