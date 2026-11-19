//! `ed projects` and `ed project save|saveas|load` (ADR 0289, slice 3): the
//! box's own card work, queued for the UI pass and run by `card_work` under
//! BUSY. A save goes over the loaded project's own file (MENU hold's), a
//! save-as into a new id, a load witnessed by the listing's generation and
//! refused over unsaved edits unless the editor says `discard` after its
//! own confirm. Never a rename, a clear, a delete or a format.

use core::fmt::{self, Write};

use super::lit::lit;
use crate::name::{ProjectName, is_name_byte};
use crate::project::ProjectStatus;
use crate::storage::{Generation, ProjectId};
use crate::ui::UiState;

/// A space in a name, on the wire: one byte NAMING never types, so a name
/// is one word and reads back as the box wrote it.
pub const SPACE: u8 = b'_';

const _: () = assert!(!is_name_byte(SPACE));

/// A name `saveas` takes: a project name as NAMING would make it, written
/// as one word with `SPACE` for each space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdName(ProjectName);

impl EdName {
    #[cfg_attr(
        target_os = "none",
        unsafe(link_section = ".bank2_ed.project.parse_name")
    )]
    pub fn parse(w: &[u8]) -> Option<EdName> {
        let mut b = [0u8; crate::ui::settings::naming::NAME_MAX];
        let to = b.get_mut(..w.len())?;
        for (o, &c) in to.iter_mut().zip(w) {
            *o = if c == SPACE { b' ' } else { c };
        }
        let s = core::str::from_utf8(to).ok()?;
        ProjectName::new(s).ok().map(EdName)
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.project.name"))]
    pub fn get(self) -> ProjectName {
        self.0
    }
}

/// A load over a project with unsaved edits: refused, or the editor's own
/// confirm said to drop them (`discard`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverEdits {
    Refuse,
    Discard,
}

/// One project command, typed at the boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectCmd {
    /// MENU hold's quick save, over the loaded project's own file.
    Save,
    /// SAVE PROJECT AS into a new id, never over another project.
    SaveAs(EdName),
    /// LOAD of a listed file, witnessed by its generation as `since`'s
    /// `proj` line said it.
    Load {
        id: ProjectId,
        generation: Generation,
        over: OverEdits,
    },
}

/// Why `ed project` can't be one: each its own `ERR`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectRefusal {
    /// `ERR ed project takes save, saveas <name> or load <id> <gen> [discard]`.
    Usage,
    /// A name NAMING couldn't type.
    Name,
}

impl ProjectCmd {
    /// `save`, `saveas <name>`, `load <id> <gen> [discard]`.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.project.parse"))]
    pub fn parse(mut words: crate::console::Words<'_>) -> Result<ProjectCmd, ProjectRefusal> {
        let sub = words.next().ok_or(ProjectRefusal::Usage)?;
        let is = |name: &[u8]| sub.eq_ignore_ascii_case(name);
        let cmd = if is(SAVE.bytes()) {
            ProjectCmd::Save
        } else if is(SAVEAS.bytes()) {
            let n = words.next().ok_or(ProjectRefusal::Usage)?;
            ProjectCmd::SaveAs(EdName::parse(n).ok_or(ProjectRefusal::Name)?)
        } else if is(LOAD.bytes()) {
            // A match, not `and_then(uint)`: that instance's name matches
            // the tuning group's glob, S0-S3's.
            let n = |w: Option<&[u8]>| match w {
                Some(w) => chimera_theory::tuning::uint(w),
                None => None,
            };
            let id = n(words.next()).and_then(ProjectId::new);
            let generation = n(words.next()).map(Generation::new);
            let over = match words.next() {
                None => OverEdits::Refuse,
                Some(w) if w.eq_ignore_ascii_case(DISCARD.bytes()) => OverEdits::Discard,
                Some(_) => return Err(ProjectRefusal::Usage),
            };
            match (id, generation) {
                (Some(id), Some(generation)) => ProjectCmd::Load {
                    id,
                    generation,
                    over,
                },
                (Some(_) | None, Some(_) | None) => return Err(ProjectRefusal::Usage),
            }
        } else {
            return Err(ProjectRefusal::Usage);
        };
        match words.next() {
            None => Ok(cmd),
            Some(_) => Err(ProjectRefusal::Usage),
        }
    }
}

/// Why the box wouldn't take `c` now, said at once (the UI pass checks it
/// again, and a stale witness writes nothing): a save with no file of its
/// own, a load of a file the listing doesn't hold at that generation, one
/// it can't load, or over edits without `discard`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotNow {
    NoFile,
    NotListed,
    Unloadable,
    Edited,
}

impl NotNow {
    /// `ERR …`, whole.
    #[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.project.words"))]
    pub fn write(self, w: &mut dyn Write) -> fmt::Result {
        w.write_str(match self {
            NotNow::NoFile => NO_FILE.get(),
            NotNow::NotListed => NOT_LISTED.get(),
            NotNow::Unloadable => UNLOADABLE.get(),
            NotNow::Edited => EDITED.get(),
        })
    }
}

/// What `c` would do on `ui` now, or why not.
#[cfg_attr(target_os = "none", unsafe(link_section = ".bank2_ed.project.check"))]
pub fn check(c: ProjectCmd, ui: &UiState) -> Result<(), NotNow> {
    match c {
        ProjectCmd::Save => match ui.project().meta().file() {
            Some(_) => Ok(()),
            None => Err(NotNow::NoFile),
        },
        ProjectCmd::SaveAs(_) => Ok(()),
        ProjectCmd::Load {
            id,
            generation,
            over,
        } => {
            let e = crate::ui::ed_cue::listed(ui, id, generation).ok_or(NotNow::NotListed)?;
            if e.err.is_some() {
                return Err(NotNow::Unloadable);
            }
            match (over, ui.project_status()) {
                (OverEdits::Refuse, ProjectStatus::Modified) => Err(NotNow::Edited),
                (OverEdits::Refuse, ProjectStatus::Pristine | ProjectStatus::Saved)
                | (OverEdits::Discard, _) => Ok(()),
            }
        }
    }
}

lit!(SAVE = "save");
lit!(SAVEAS = "saveas");
lit!(LOAD = "load");
lit!(DISCARD = "discard");
lit!(
    /// The usage refusal.
    pub(super) USAGE = "ERR ed project takes save, saveas <name> or load <id> <gen> [discard]\n"
);
lit!(
    pub(super) NAME_USAGE = "ERR ed project saveas takes a name of A-Z a-z 0-9 - and _ for a space, 1-16\n"
);
lit!(NO_FILE = "ERR ed project save has no file of its own: saveas\n");
lit!(NOT_LISTED = "ERR ed project load has no such id at that gen: ed projects\n");
lit!(UNLOADABLE = "ERR ed project load can't read that file\n");
lit!(EDITED = "ERR ed project load over unsaved edits: discard\n");
