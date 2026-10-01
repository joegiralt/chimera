//! The SETTINGS tree: rows with a kind, one static table. A `Later` row is in the
//! table, so a feature is built out by replacing its `Later(issue(n))`
//! kind and navigation stays as it is.

use super::leaves::{
    ABOUT_LEAF, CHANNELS_LEAF, OUTPUTS_LEAF, THEME_LEAF, TUNING_LEAF, UPDATES_LEAF,
};
use crate::ui::block_def::ChainDef2;
use core::num::NonZeroU16;

#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub label: &'static str,
    /// The breadcrumb's name for this row, at most 8 characters.
    pub crumb: &'static str,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    List(&'static [Row]),
    Leaf(&'static ChainDef2),
    /// A screen whose rows are built at run time.
    Screen(Screen),
    Act(Act),
    /// Not built: dimmed, and EDIT and SEQ ignore it.
    Later(Issue),
}

/// A GitHub issue number: never 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Issue(NonZeroU16);

impl Issue {
    pub fn get(self) -> u16 {
        self.0.get()
    }
}

pub const fn issue(n: u16) -> Issue {
    match NonZeroU16::new(n) {
        Some(n) => Issue(n),
        None => panic!("issue number 0"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    LoadProject,
    ManageProjects,
    /// OVER SLOT nn, TO NEW SLOT nn: slot numbers are run-time.
    SaveToProj,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    SaveProjectAs,
    PartRename,
    PartClear,
    PartReload,
}

const fn row(label: &'static str, kind: Kind) -> Row {
    Row {
        label,
        crumb: label,
        kind,
    }
}

const fn crumb(label: &'static str, crumb: &'static str, kind: Kind) -> Row {
    Row { label, crumb, kind }
}

use Kind::{Later, Leaf, List};

static PROJECT: [Row; 3] = [
    crumb("LOAD PROJECT", "LOAD", Kind::Screen(Screen::LoadProject)),
    crumb("SAVE PROJECT AS", "SAVE AS", Kind::Act(Act::SaveProjectAs)),
    crumb(
        "MANAGE PROJECTS",
        "MANAGE",
        Kind::Screen(Screen::ManageProjects),
    ),
];

static PART: [Row; 4] = [
    row("RENAME", Kind::Act(Act::PartRename)),
    row("CLEAR", Kind::Act(Act::PartClear)),
    crumb("SAVE TO PROJ", "SAVE TO", Kind::Screen(Screen::SaveToProj)),
    crumb("RELOAD FROM PROJ", "RELOAD", Kind::Act(Act::PartReload)),
];

static MIDI: [Row; 3] = [
    row("SYNC", Later(issue(264))),
    crumb("PORT CONFIG", "PORT", Later(issue(265))),
    row("CHANNELS", Leaf(&CHANNELS_LEAF)),
];

static AUDIO: [Row; 3] = [
    row("OUTPUTS", Leaf(&OUTPUTS_LEAF)),
    row("SENDS", Later(issue(259))),
    row("TUNING", Leaf(&TUNING_LEAF)),
];

static PERSONALIZE: [Row; 1] = [row("THEME", Leaf(&THEME_LEAF))];

static SYSTEM: &[Row] = &[
    crumb("OS UPGRADE", "OS", Leaf(&UPDATES_LEAF)),
    row("STORAGE", Later(issue(267))),
    crumb("FORMAT CARD", "FORMAT", Later(issue(268))),
    crumb("USB CONFIG", "USB", Later(issue(269))),
    row("ABOUT", Leaf(&ABOUT_LEAF)),
    #[cfg(debug_assertions)]
    row("DEMO", Leaf(&crate::ui::block_registry::DEMO_CHAIN)),
];

static TOP: [Row; 8] = [
    row("PROJECT", List(&PROJECT)),
    row("PART", List(&PART)),
    row("ORBIT", Later(issue(262))),
    crumb("MIDI CONFIG", "MIDI", List(&MIDI)),
    crumb("SYSEX DUMP", "SYSEX", Later(issue(263))),
    crumb("AUDIO ROUTING", "AUDIO", List(&AUDIO)),
    crumb("PERSONALIZE", "PERSONAL", List(&PERSONALIZE)),
    row("SYSTEM", List(SYSTEM)),
];

pub static ROOT: Row = row("SETTINGS", List(&TOP));

/// The row a path of row indices names; the empty path is `ROOT`.
pub fn row_at(path: &[u8]) -> Option<&'static Row> {
    let mut cur = &ROOT;
    for &i in path {
        match cur.kind {
            List(rows) => cur = rows.get(i as usize)?,
            _ => return None,
        }
    }
    Some(cur)
}

/// A list's rows; empty for anything else.
pub fn rows(path: &[u8]) -> &'static [Row] {
    match row_at(path).map(|r| r.kind) {
        Some(List(rows)) => rows,
        _ => &[],
    }
}
