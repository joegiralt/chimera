//! Typed indices: a Part, a pool slot, a set of Parts. Only `new` makes an
//! id, so every one is in range.

use crate::hw::MAX_PARTS;
use crate::preset::POOL_SIZE;

/// One of the six Parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PartId(u8);

impl PartId {
    pub const ALL: [PartId; MAX_PARTS] = {
        let mut a = [PartId(0); MAX_PARTS];
        let mut i = 0;
        while i < MAX_PARTS {
            a[i] = PartId(i as u8);
            i += 1;
        }
        a
    };

    pub const fn new(n: u8) -> Option<PartId> {
        if (n as usize) < MAX_PARTS {
            Some(PartId(n))
        } else {
            None
        }
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// One of the pool's slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SlotId(u8);

impl SlotId {
    pub const ALL: [SlotId; POOL_SIZE] = {
        let mut a = [SlotId(0); POOL_SIZE];
        let mut i = 0;
        while i < POOL_SIZE {
            a[i] = SlotId(i as u8);
            i += 1;
        }
        a
    };

    pub const fn new(n: u8) -> Option<SlotId> {
        if (n as usize) < POOL_SIZE {
            Some(SlotId(n))
        } else {
            None
        }
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// A set of Parts, a bit each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PartSet(u8);

const _: () = assert!(MAX_PARTS <= u8::BITS as usize);

impl PartSet {
    pub const EMPTY: PartSet = PartSet(0);

    pub fn with(self, p: PartId) -> PartSet {
        PartSet(self.0 | 1 << p.0)
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
