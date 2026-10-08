//! A value that says when it may have changed (ADR 0112, 0115): each
//! Part's Sound is one, so the Part's knobs look at a Sound again only when
//! that Sound was written, not on any edit to the Project.

use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicU32, Ordering};

/// Serials handed out, one a fresh value or a write: never the same twice
/// (until 2³² of them), whichever copy draws it.
static SERIAL: AtomicU32 = AtomicU32::new(1);

/// A value's serial: drawn when it was made or last written, so no other
/// value has it, and copied with it (a publish carries its Sound's). Equal
/// stamps, equal values. Only this crate can name one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    serial: u32,
}

impl Stamp {
    /// No value's: serials start at 1.
    pub(crate) const NONE: Self = Self { serial: 0 };

    /// One no other value has had.
    fn unique() -> Self {
        Self {
            serial: SERIAL.fetch_add(1, Ordering::Relaxed),
        }
    }
}

/// `T` and its `Stamp`. Read as a `T`; any `&mut` to it counts a write, so
/// no change slips past the stamp:
///
/// ```
/// use chimera_engine::tracked::Tracked;
/// let mut t = Tracked::fresh(1.0f32);
/// let s = t.stamp();
/// *t += 1.0;
/// assert_ne!(t.stamp(), s);
/// ```
///
/// and no stamp can be carried to a new value from outside:
///
/// ```compile_fail
/// use chimera_engine::tracked::Tracked;
/// let t = Tracked::fresh(1.0f32);
/// let forged = Tracked::new(2.0f32, t.stamp());
/// ```
#[derive(Clone)]
pub struct Tracked<T> {
    value: T,
    stamp: Stamp,
}

impl<T> Tracked<T> {
    /// `value` as published at `stamp`: the Instrument's publish only.
    pub(crate) const fn new(value: T, stamp: Stamp) -> Self {
        Self { value, stamp }
    }

    /// `value` with a stamp no other value has: so a stamp can't be
    /// carried over to a new value.
    pub fn fresh(value: T) -> Self {
        Self {
            value,
            stamp: Stamp::unique(),
        }
    }

    pub fn stamp(&self) -> Stamp {
        self.stamp
    }
}

/// The value alone: its stamp is bookkeeping, not what it holds.
impl<T: core::fmt::Debug> core::fmt::Debug for Tracked<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.value.fmt(f)
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
        self.stamp = Stamp::unique();
        &mut self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_written_apart_never_share_a_stamp() {
        let a = Tracked::fresh(0u8);
        let (mut b, mut c) = (a.clone(), a.clone());
        assert_eq!(b.stamp(), c.stamp());
        (*b, *c) = (1, 2);
        assert_ne!(b.stamp(), c.stamp());
        assert_ne!(b.stamp(), a.stamp());
        assert_ne!(Tracked::fresh(0u8).stamp(), Tracked::fresh(0u8).stamp());
    }
}
