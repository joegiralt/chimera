// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! SETTINGS › PART's actions on the active Part: one `Offer`, from
//! `part_actions`, that both the dimming and the run read.

use core::fmt::Write;

use crate::project::arena::Cost;
use crate::project::{
    Line, Origin, PartAction, PartActionKind, PartId, Project, SlotId, SoundStatus, part_actions,
};

use super::prompt::Slot;
use super::tree::Act;
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

    /// Its place in an `Offer`.
    const fn idx(self) -> usize {
        match self {
            PartCmd::OverSlot => 0,
            PartCmd::NewSlot => 1,
            PartCmd::Reload => 2,
        }
    }
}

impl Act {
    /// The PART row that runs a Part action: dimmed and run by its `Offer`.
    pub const fn part_cmd(self) -> Option<PartCmd> {
        match self {
            Act::PartReload => Some(PartCmd::Reload),
            Act::SaveProjectAs
            | Act::PartRename
            | Act::PartClear
            | Act::SaveToLibrary
            | Act::SceneClear
            | Act::SceneReload
            | Act::EnterDfu
            | Act::PurgeUnusedSources => None,
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
    /// An Edited Part's save over its slot won't fit.
    over_full: bool,
    /// What the Part's save takes, offered or not (`PartActions::save_needs`).
    save_needs: Cost,
}

impl Offer {
    pub fn of(p: &Project, part: PartId) -> Self {
        let offered = part_actions(p, part);
        let mut actions = [None; 3];
        for a in offered.iter() {
            actions[PartCmd::of(a.kind()).idx()] = Some(a);
        }
        let home = match p.part(part).origin() {
            Origin::Slot { slot, .. } => Some(slot),
            Origin::Init(_) => None,
        };
        let over_full = actions[PartCmd::OverSlot.idx()].is_none()
            && offered.status() == SoundStatus::Edited
            && home.is_some();
        Offer {
            actions,
            home,
            over_full,
            save_needs: offered.save_needs(),
        }
    }

    pub fn get(&self, c: PartCmd) -> Option<PartAction> {
        self.actions[c.idx()]
    }

    /// SAVE PART FIRST's save: over the slot, else to a new one.
    pub fn first_save(&self) -> Option<PartAction> {
        SAVE_ROWS.iter().find_map(|&c| self.get(c))
    }

    /// What MIX + EDIT's save takes, if one is offered: `first_save`'s.
    pub fn save_cost(&self) -> Option<Cost> {
        self.first_save().map(|_| self.save_needs)
    }

    /// What the Part's save would take, offered or not: a refusal's NEEDS.
    pub fn save_needs(&self) -> Cost {
        self.save_needs
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

    /// A save the pool has no room for: no new slot is ever withheld for
    /// another reason.
    fn full(&self, c: PartCmd) -> bool {
        match c {
            PartCmd::OverSlot => self.over_full,
            PartCmd::NewSlot => self.get(c).is_none(),
            PartCmd::Reload => false,
        }
    }

    /// A SAVE TO PROJ row: `OVER SLOT 03`, `TO NEW SLOT 09`; `POOL FULL`
    /// in place of a save that won't fit (ADR 0043).
    pub fn label(&self, c: PartCmd) -> Line {
        if self.full(c) {
            return Line::new(super::prompt::POOL_FULL);
        }
        let mut l = Line::new(match c {
            PartCmd::OverSlot => "OVER ",
            PartCmd::NewSlot => "TO NEW ",
            PartCmd::Reload => "",
        });
        let _ = write_slot(&mut l, self.slot(c));
        l
    }

    /// Bytes for a region key: each row's slot, then whether it is live
    /// and whether the pool is full for it.
    pub fn key(&self) -> [u8; 6] {
        let mut k = [0; 6];
        for (i, c) in [PartCmd::OverSlot, PartCmd::NewSlot, PartCmd::Reload]
            .into_iter()
            .enumerate()
        {
            k[2 * i] = self.slot(c).map_or(0, |s| s.index() as u8 + 1);
            k[2 * i + 1] = self.get(c).is_some() as u8 | (self.full(c) as u8) << 1;
        }
        k
    }
}

/// `SLOT 03`, or `SLOT --`.
fn write_slot(l: &mut Line, s: Option<SlotId>) -> core::fmt::Result {
    match s {
        Some(s) => write!(l, "{}", Slot(s)),
        None => l.write_str("SLOT --"),
    }
}

/// The slot an action saves to or reverts from.
pub fn slot_of(k: PartActionKind) -> SlotId {
    match k {
        PartActionKind::OverSlot(s) | PartActionKind::NewSlot(s) | PartActionKind::Revert(s) => s,
    }
}
