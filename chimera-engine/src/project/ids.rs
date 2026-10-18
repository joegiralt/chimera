//! Typed indices: a Part, a pool slot, a set of Parts. Only `new` makes an
//! id, so every one is in range.

use crate::hw::MAX_PARTS;
use crate::preset::MAX_SOUNDS;

/// A `u8` index below `$len`, only `new` (or `ALL`) making one: the
/// struct, `ALL` in order, and `new`. The type adds its own reader.
macro_rules! bounded_index {
    ($(#[$m:meta])* $name:ident < $len:expr) => {
        $(#[$m])*
        pub struct $name(u8);

        impl $name {
            pub const ALL: [$name; $len] = {
                let mut a = [$name(0); $len];
                let mut i = 0;
                while i < $len {
                    a[i] = $name(i as u8);
                    i += 1;
                }
                a
            };

            pub const fn new(n: u8) -> Option<$name> {
                if (n as usize) < $len {
                    Some($name(n))
                } else {
                    None
                }
            }
        }
    };
}
pub(crate) use bounded_index;

bounded_index!(
    /// One of the six Parts.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    PartId < MAX_PARTS
);

impl PartId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

bounded_index!(
    /// One of the pool's slots.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    SlotId < MAX_SOUNDS
);

impl SlotId {
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A set of Parts, a bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct PartSet(u8);

const _: () = assert!(MAX_PARTS <= u8::BITS as usize);

impl PartSet {
    pub const EMPTY: PartSet = PartSet(0);

    pub fn with(self, p: PartId) -> PartSet {
        PartSet(self.0 | 1 << p.0)
    }

    /// The bits, for an atomic word.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// `bits` back, the bits past the Parts dropped.
    pub const fn from_bits(b: u8) -> PartSet {
        PartSet(b & ((1u16 << MAX_PARTS) - 1) as u8)
    }

    pub fn contains(self, p: PartId) -> bool {
        self.0 & 1 << p.0 != 0
    }

    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// In Part order.
    pub fn iter(self) -> impl Iterator<Item = PartId> {
        PartId::ALL.into_iter().filter(move |&p| self.contains(p))
    }
}
