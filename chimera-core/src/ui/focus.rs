//! The last-touched encoder slot per page (UI refresh spec § Focus
//! tracking): the focus band shows it until another slot is touched. No
//! timer. Pages are keyed by `BlockDef::id`, so a page shared by several
//! chains (FILTER) keeps one focus, and the FM operator selection does not
//! reset it.

/// One entry per `BlockDef::id`; ids are 0..=40 today (test-checked).
pub const MAX_PAGES: usize = 48;

#[derive(Clone, Copy, Debug)]
pub struct FocusMemory {
    slots: [u8; MAX_PAGES],
}

impl Default for FocusMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusMemory {
    /// Every page starts focused on slot a.
    pub const fn new() -> Self {
        Self { slots: [0; MAX_PAGES] }
    }

    /// The focused slot (0..=5) of page `def_id`.
    pub fn get(&self, def_id: u16) -> usize {
        self.slots.get(def_id as usize).map_or(0, |&s| s as usize)
    }

    /// Slot `slot` of page `def_id` was just turned.
    pub fn touch(&mut self, def_id: u16, slot: usize) {
        if let Some(s) = self.slots.get_mut(def_id as usize) {
            *s = slot.min(5) as u8;
        }
    }
}
