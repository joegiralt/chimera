//! SETTINGS › PART's actions on the active Part: one `Offer`, from
//! `part_actions`, that both the dimming and the run read.

use core::fmt::Write;

use crate::project::{
    Line, Origin, PartAction, PartActionKind, PartId, Project, SlotId, part_actions,
};

use super::view::RowLook;

/// A row that runs a Part action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartCmd {
    OverSlot,
    NewSlot,
    /// RELOAD FROM PROJ: `Revert`.
    Reload,
}

/// SAVE TO PROJ's rows, top to bottom.
pub const SAVE_ROWS: [PartCmd; 2] = [PartCmd::OverSlot, PartCmd::NewSlot];

impl PartCmd {
    fn of(k: PartActionKind) -> Self {
        match k {
            PartActionKind::OverSlot(_) => PartCmd::OverSlot,
            PartActionKind::NewSlot(_) => PartCmd::NewSlot,
            PartActionKind::Revert(_) => PartCmd::Reload,
        }
    }
}

/// What `part_actions` offers a Part, by row: a row is live only with its
/// action. Only `of` makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    actions: [Option<PartAction>; 3],
    /// The Part's slot, named on a row that doesn't apply.
    home: Option<SlotId>,
}

impl Offer {
    pub fn of(p: &Project, part: PartId) -> Self {
        let mut actions = [None; 3];
        for a in part_actions(p, part).iter() {
            actions[PartCmd::of(a.kind()) as usize] = Some(a);
        }
        let home = match p.part(part).origin() {
            Origin::Slot { slot, .. } => Some(slot),
            Origin::Init(_) => None,
        };
        Offer { actions, home }
    }

    pub fn get(&self, c: PartCmd) -> Option<PartAction> {
        self.actions[c as usize]
    }

    /// SAVE PART FIRST's save: over the slot, else to a new one.
    pub fn first_save(&self) -> Option<PartAction> {
        SAVE_ROWS.iter().find_map(|&c| self.get(c))
    }

    pub fn look(&self, c: PartCmd) -> RowLook {
        match self.get(c) {
            Some(_) => RowLook::Normal,
            None => RowLook::Dimmed,
        }
    }

    /// The slot `c` acts on; a dimmed row names the Part's own.
    pub fn slot(&self, c: PartCmd) -> Option<SlotId> {
        match self.get(c) {
            Some(a) => Some(slot_of(a.kind())),
            None if c == PartCmd::NewSlot => None,
            None => self.home,
        }
    }

    /// A SAVE TO PROJ row: `OVER SLOT 03`, `TO NEW SLOT 09`; `--` for none.
    pub fn label(&self, c: PartCmd) -> Line {
        let mut l = Line::new(match c {
            PartCmd::OverSlot => "OVER ",
            PartCmd::NewSlot => "TO NEW ",
            PartCmd::Reload => "",
        });
        let _ = write_slot(&mut l, self.slot(c));
        l
    }

    /// Bits for a region key: which rows are live, and their slots.
    pub fn key(&self) -> [u8; 3] {
        [PartCmd::OverSlot, PartCmd::NewSlot, PartCmd::Reload].map(|c| {
            self.slot(c).map_or(0, |s| s.index() as u8 + 1) | (self.get(c).is_some() as u8) << 7
        })
    }
}

/// `SLOT 03`, or `SLOT --`.
fn write_slot(l: &mut Line, s: Option<SlotId>) -> core::fmt::Result {
    match s {
        Some(s) => write!(l, "SLOT {:02}", s.index() + 1),
        None => l.write_str("SLOT --"),
    }
}

/// The slot an action saves to or reverts from.
pub fn slot_of(k: PartActionKind) -> SlotId {
    match k {
        PartActionKind::OverSlot(s) | PartActionKind::NewSlot(s) | PartActionKind::Revert(s) => s,
    }
}
