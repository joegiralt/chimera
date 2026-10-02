//! MANAGE PROJECTS' commands, and what each does to the project under the
//! bar, loaded or not: one table, `Command::on`, that both the dimming and
//! the run read.

use crate::project::{ProjectEntry, ProjectFile};

use super::tree::{Issue, issue};
use super::view::{ListRow, RowLook};

/// The command column, top to bottom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    LoadFrom,
    SaveTo,
    Rename,
    Clear,
    Delete,
    Protect,
}

pub const MANAGE_COMMANDS: [Command; 6] = [
    Command::LoadFrom,
    Command::SaveTo,
    Command::Rename,
    Command::Clear,
    Command::Delete,
    Command::Protect,
];
const _: () = {
    let mut i = 0;
    while i < MANAGE_COMMANDS.len() {
        assert!(MANAGE_COMMANDS[i] as usize == i, "in declaration order");
        i += 1;
    }
};

/// Why a command is dimmed: the legend says it, as the column has no room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// RENAME of a project not loaded (#273, Pre-flight 9).
    LoadToRename,
}

impl Note {
    pub fn text(self) -> &'static str {
        match self {
            Note::LoadToRename => "LOAD TO RENAME",
        }
    }
}

const PROTECT_ISSUE: Issue = issue(270);

/// The project under the bar: the one in RAM, or another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Whose {
    Loaded,
    Other,
}

impl Whose {
    pub fn of(e: &ProjectEntry, loaded: Option<ProjectFile>) -> Self {
        if Some(e.file()) == loaded {
            Whose::Loaded
        } else {
            Whose::Other
        }
    }
}

/// What a command that applies does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run {
    /// As LOAD.
    Load,
    /// SAVE OVER, then `Save(Over)`.
    SaveOver,
    /// NAMING, in RAM.
    RenameLoaded,
    /// CLEAR, then its own: a guarded NEW saved over its file; another:
    /// `Job::Clear`.
    Clear(Whose),
    /// DELETE, then `Job::Delete`.
    Delete,
}

/// Why a command doesn't apply: dimmed with its note, or not built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Off {
    Dimmed(Option<Note>),
    Later(Issue),
}

impl Command {
    pub fn at(i: u8) -> Option<Command> {
        MANAGE_COMMANDS.get(i as usize).copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            Command::LoadFrom => "LOAD FROM",
            Command::SaveTo => "SAVE TO",
            Command::Rename => "RENAME",
            Command::Clear => "CLEAR",
            Command::Delete => "DELETE",
            Command::Protect => "PROTECT",
        }
    }

    /// The table: what `self` does on `w`'s project, or why it can't.
    pub const fn on(self, w: Whose) -> Result<Run, Off> {
        use Command as C;
        match (self, w) {
            (C::LoadFrom, _) => Ok(Run::Load),
            (C::SaveTo, _) => Ok(Run::SaveOver),
            (C::Rename, Whose::Loaded) => Ok(Run::RenameLoaded),
            (C::Rename, Whose::Other) => Err(Off::Dimmed(Some(Note::LoadToRename))),
            (C::Clear, w) => Ok(Run::Clear(w)),
            (C::Delete, Whose::Loaded) => Err(Off::Dimmed(None)),
            (C::Delete, Whose::Other) => Ok(Run::Delete),
            (C::Protect, _) => Err(Off::Later(PROTECT_ISSUE)),
        }
    }

    /// As drawn on `w`'s project; with none listed, dimmed.
    pub fn row(self, w: Option<Whose>) -> ListRow<'static> {
        let look = match w.map(|w| self.on(w)) {
            Some(Ok(_)) => RowLook::Normal,
            Some(Err(Off::Later(_))) => RowLook::Later,
            Some(Err(Off::Dimmed(_))) | None => RowLook::Dimmed,
        };
        ListRow {
            label: self.label(),
            opens: false,
            note: None,
            look,
        }
    }
}

/// The column as drawn on `w`'s project.
pub fn command_rows(w: Option<Whose>) -> [ListRow<'static>; 6] {
    MANAGE_COMMANDS.map(|c| c.row(w))
}
