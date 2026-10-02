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

/// MANAGE PROJECTS' command column (spec § MANAGE PROJECTS); PROTECT is later.
pub const MANAGE_COMMANDS: [&str; 6] = [
    "LOAD FROM",
    "SAVE TO",
    "RENAME",
    "CLEAR",
    "DELETE",
    "PROTECT",
];

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

/// The active Part's list: its crumb is `PART n`.
pub static PART: [Row; 4] = [
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

/// The top list's PART row, found by its crumb at compile time: a tree
/// without one fails the build.
pub const PART_ROW: u8 = {
    let mut i = 0;
    loop {
        assert!(i < TOP.len(), "no PART row at the top");
        if str_eq(TOP[i].crumb, "PART") && matches!(TOP[i].kind, List(_)) {
            break i as u8;
        }
        i += 1;
    }
};

const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

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

/// The PROJECT list's place at the top, and SAVE PROJECT AS's row in it,
/// found at compile time.
pub const SAVE_AS_AT: (u8, u8) = {
    let mut i = 0;
    loop {
        assert!(i < TOP.len(), "no SAVE PROJECT AS under the top");
        if let List(rs) = TOP[i].kind {
            let mut j = 0;
            while j < rs.len() {
                if matches!(rs[j].kind, Kind::Act(Act::SaveProjectAs)) {
                    break;
                }
                j += 1;
            }
            if j < rs.len() {
                break (i as u8, j as u8);
            }
        }
        i += 1;
    }
};

/// The kind of the row `path` names, at compile time.
pub const fn kind_at(path: &[u8]) -> Option<Kind> {
    let mut cur = &ROOT;
    let mut d = 0;
    while d < path.len() {
        match cur.kind {
            List(rows) if (path[d] as usize) < rows.len() => cur = &rows[path[d] as usize],
            _ => return None,
        }
        d += 1;
    }
    Some(cur.kind)
}

/// The path to Screen `s`'s row and its depth; no such row fails the build
/// where a const asks.
pub const fn path_of(s: Screen) -> ([u8; 4], usize) {
    const fn find(
        rows: &'static [Row],
        s: Screen,
        path: [u8; 4],
        d: usize,
    ) -> Option<([u8; 4], usize)> {
        if d == path.len() {
            return None;
        }
        let mut i = 0;
        while i < rows.len() {
            let mut p = path;
            p[d] = i as u8;
            match rows[i].kind {
                Kind::Screen(t) if t as u8 == s as u8 => return Some((p, d + 1)),
                List(rs) => {
                    if let Some(hit) = find(rs, s, p, d + 1) {
                        return Some(hit);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }
    match find(&TOP, s, [0; 4], 0) {
        Some(hit) => hit,
        None => panic!("a Screen with no row"),
    }
}

/// Each Screen's path, found at compile time.
const SCREEN_PATHS: [([u8; 4], usize); 3] = [
    path_of(Screen::LoadProject),
    path_of(Screen::ManageProjects),
    path_of(Screen::SaveToProj),
];

/// Screen `s`'s path in the tree.
pub fn screen_path(s: Screen) -> &'static [u8] {
    let (path, depth) = match s {
        Screen::LoadProject => &SCREEN_PATHS[0],
        Screen::ManageProjects => &SCREEN_PATHS[1],
        Screen::SaveToProj => &SCREEN_PATHS[2],
    };
    &path[..*depth]
}
