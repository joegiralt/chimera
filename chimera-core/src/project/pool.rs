//! The project's Sounds: 32 slots, each with a generation that every store
//! and clear moves, so a Part can tell its slot was replaced.

use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use crate::preset::{POOL_SIZE, Sound};

use super::ids::SlotId;

/// Only `project` stores or clears, so only it moves a generation:
///
/// ```compile_fail,E0624
/// use chimera_core::preset::Sound;
/// use chimera_core::project::{Pool, SlotId};
/// fn f(pool: &mut Pool, s: SlotId, sound: Sound) {
///     pool.store(s, sound);
/// }
/// ```
pub struct Pool {
    pub(in crate::project) slots: [Option<Sound>; POOL_SIZE],
    pub(in crate::project) gens: [u16; POOL_SIZE],
}

impl Pool {
    /// Empty, every generation 0, built in place: the 28 KB pool never
    /// passes through the stack.
    pub fn init_in_place(slot: &mut MaybeUninit<Pool>) -> &mut Pool {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` is valid and unaliased; every slot and the
        // generations are written once before `assume_init_mut`.
        unsafe {
            let slots = addr_of_mut!((*p).slots).cast::<Option<Sound>>();
            for i in 0..POOL_SIZE {
                slots.add(i).write(None);
            }
            addr_of_mut!((*p).gens).write([0; POOL_SIZE]);
            slot.assume_init_mut()
        }
    }

    pub fn get(&self, s: SlotId) -> Option<&Sound> {
        self.slots[s.index()].as_ref()
    }

    pub fn generation(&self, s: SlotId) -> u16 {
        self.gens[s.index()]
    }

    pub fn first_free(&self) -> Option<SlotId> {
        SlotId::ALL.into_iter().find(|&s| self.get(s).is_none())
    }

    pub fn used(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }

    pub(in crate::project) fn store(&mut self, s: SlotId, sound: Sound) {
        self.slots[s.index()] = Some(sound);
        self.bump(s);
    }

    /// Moves the generation even when the slot is already empty.
    pub(in crate::project) fn clear(&mut self, s: SlotId) {
        self.slots[s.index()] = None;
        self.bump(s);
    }

    fn bump(&mut self, s: SlotId) {
        let g = &mut self.gens[s.index()];
        *g = g.wrapping_add(1);
    }
}
