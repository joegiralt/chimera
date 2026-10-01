//! The SETTINGS tree: rows with a kind and a status, one static table.
//! A `Later` row is in the table, so a feature flips its status and
//! navigation stays as it is.

use super::leaves::*;
use crate::ui::block_def::ChainDef2;

#[derive(Clone, Copy, Debug)]
pub struct Row {
    pub label: &'static str,
    /// The breadcrumb's name for this row, at most 8 characters.
    pub crumb: &'static str,
    pub kind: Kind,
    pub status: Status,
}

#[derive(Clone, Copy, Debug)]
pub enum Kind {
    List(&'static [Row]),
    Leaf(&'static ChainDef2),
    /// A screen whose rows are built at run time.
    Screen(Screen),
    Act(Act),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Built,
    /// An existing page moved here.
    Mirror,
    /// Not built: the GitHub issue number.
    Later(u16),
}

const fn row(label: &'static str, kind: Kind, status: Status) -> Row {
    Row {
        label,
        crumb: label,
        kind,
        status,
    }
}

const fn crumb(label: &'static str, crumb: &'static str, kind: Kind, status: Status) -> Row {
    Row {
        label,
        crumb,
        kind,
        status,
    }
}

use Kind::{Act as A, Leaf, List, Screen as S};
use Status::{Built, Later, Mirror};

static PROJECT: [Row; 3] = [
    crumb("LOAD PROJECT", "LOAD", S(Screen::LoadProject), Built),
    crumb("SAVE PROJECT AS", "SAVE AS", A(Act::SaveProjectAs), Built),
    crumb(
        "MANAGE PROJECTS",
        "MANAGE",
        S(Screen::ManageProjects),
        Built,
    ),
];

static PART: [Row; 4] = [
    row("RENAME", A(Act::PartRename), Built),
    row("CLEAR", A(Act::PartClear), Built),
    crumb("SAVE TO PROJ", "SAVE TO", S(Screen::SaveToProj), Built),
    crumb("RELOAD FROM PROJ", "RELOAD", A(Act::PartReload), Built),
];

static MIDI: [Row; 3] = [
    row("SYNC", List(&[]), Later(264)),
    crumb("PORT CONFIG", "PORT", List(&[]), Later(265)),
    row("CHANNELS", Leaf(&CHANNELS_LEAF), Mirror),
];

static AUDIO: [Row; 3] = [
    row("OUTPUTS", Leaf(&OUTPUTS_LEAF), Mirror),
    row("SENDS", List(&[]), Later(259)),
    row("TUNING", Leaf(&TUNING_LEAF), Mirror),
];

static PERSONALIZE: [Row; 1] = [row("THEME", Leaf(&THEME_LEAF), Mirror)];

static SYSTEM: &[Row] = &[
    crumb("OS UPGRADE", "OS", Leaf(&UPDATES_LEAF), Mirror),
    row("STORAGE", List(&[]), Later(267)),
    crumb("FORMAT CARD", "FORMAT", List(&[]), Later(268)),
    crumb("USB CONFIG", "USB", List(&[]), Later(269)),
    row("ABOUT", Leaf(&ABOUT_LEAF), Mirror),
    #[cfg(debug_assertions)]
    row("DEMO", Leaf(&crate::ui::block_registry::DEMO_CHAIN), Built),
];

static TOP: [Row; 8] = [
    row("PROJECT", List(&PROJECT), Built),
    row("PART", List(&PART), Built),
    row("ORBIT", List(&[]), Later(262)),
    crumb("MIDI CONFIG", "MIDI", List(&MIDI), Built),
    crumb("SYSEX DUMP", "SYSEX", List(&[]), Later(263)),
    crumb("AUDIO ROUTING", "AUDIO", List(&AUDIO), Built),
    crumb("PERSONALIZE", "PERSONAL", List(&PERSONALIZE), Built),
    row("SYSTEM", List(SYSTEM), Built),
];

pub static ROOT: Row = row("SETTINGS", List(&TOP), Built);

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
