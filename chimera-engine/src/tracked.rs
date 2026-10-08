//! A published value that says when it may have changed (ADR 0112): the
//! Part's knobs look at a Sound again only then, so a settled Part costs a
//! compare a block, not a read of every knob.

use core::ops::{Deref, DerefMut};

/// Where a published value came from: the load epoch and the Project
/// revision it was published at (every `&mut` path to a Project bumps its
/// revision), and the writes made through it since (a test's; the shells
/// only publish). Equal stamps, equal values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    epoch: u32,
    rev: u32,
    writes: u32,
}

impl Stamp {
    pub const fn new(epoch: u32, rev: u32) -> Self {
        Self {
            epoch,
            rev,
            writes: 0,
        }
    }

    /// A publish with no Project behind it (the tests' `update_from`): a
    /// revision this value has not had.
    pub const fn next(self, epoch: u32) -> Self {
        Self::new(epoch, self.rev.wrapping_add(1))
    }
}

/// `T` and its `Stamp`. Read as a `T`; any `&mut` to it counts a write, so
/// no change slips past the stamp:
///
/// ```
/// use chimera_engine::tracked::{Stamp, Tracked};
/// let mut t = Tracked::new(1.0f32, Stamp::new(0, 0));
/// let s = t.stamp();
/// *t += 1.0;
/// assert_ne!(t.stamp(), s);
/// ```
#[derive(Clone, Debug)]
pub struct Tracked<T> {
    value: T,
    stamp: Stamp,
}

impl<T> Tracked<T> {
    pub const fn new(value: T, stamp: Stamp) -> Self {
        Self { value, stamp }
    }

    pub fn stamp(&self) -> Stamp {
        self.stamp
    }
}

impl<T> Deref for Tracked<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T> DerefMut for Tracked<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.stamp.writes = self.stamp.writes.wrapping_add(1);
        &mut self.value
    }
}
