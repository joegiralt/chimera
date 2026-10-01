//! What a project save, load, list or delete tells the screen (projects
//! spec § Errors reach the screen): one line, naming the project or its
//! file.

use core::fmt::{self, Write};

use chimera_hal::store::StoreError;

use crate::name::ProjectName;
use crate::storage::{FileError, ProjectId};

use super::{PartId, SlotId};

pub const LINE_LEN: usize = 48;

/// One line of text, cut at `LINE_LEN` bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    buf: [u8; LINE_LEN],
    len: u8,
}

impl Line {
    pub fn new(s: &str) -> Line {
        let mut l = Line {
            buf: [0; LINE_LEN],
            len: 0,
        };
        let _ = l.write_str(s);
        l
    }

    pub fn as_str(&self) -> &str {
        // Only `write_str` fills it, at char boundaries.
        core::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }
}

/// Truncating: what doesn't fit is dropped, never an error.
impl Write for Line {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let at = self.len as usize;
        let mut n = s.len().min(LINE_LEN - at);
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        self.buf[at..at + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n as u8;
        Ok(())
    }
}

/// What a note names: the project, or its file when no name is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subject {
    Name(ProjectName),
    File(ProjectId),
}

impl fmt::Display for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Subject::Name(n) => f.write_str(n.as_str()),
            Subject::File(id) => {
                let stem = id.stem();
                f.write_str(core::str::from_utf8(&stem).unwrap_or("P???????"))
            }
        }
    }
}

/// The edited Parts a save leaves differing from their slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Differ {
    None,
    One(PartId, SlotId),
    Many(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectNote {
    Saved(Differ),
    Card {
        err: StoreError,
        subject: Option<Subject>,
    },
    File {
        err: FileError,
        subject: Subject,
    },
    Missing(Subject),
    /// Pass 2 failed: the project is NEW.
    LoadFailed(Subject),
    /// The confirmed target moved: nothing ran.
    Changed,
    IsLoaded,
    NoIds,
    NewProject,
}

impl ProjectNote {
    /// Parts and slots are 1-based, slots in two digits.
    pub fn line(&self) -> Line {
        let mut l = Line::new("");
        let _ = match *self {
            ProjectNote::Saved(Differ::None) => l.write_str("SAVED"),
            ProjectNote::Saved(Differ::One(p, s)) => write!(
                l,
                "SAVED: P{} DIFFERS FROM SLOT {:02}",
                p.index() + 1,
                s.index() + 1
            ),
            ProjectNote::Saved(Differ::Many(n)) => write!(l, "SAVED: {n} PARTS DIFFER FROM SLOTS"),
            ProjectNote::Card { err, subject: None } => l.write_str(err.message()),
            ProjectNote::Card {
                err,
                subject: Some(s),
            } => write!(l, "{}: {s}", err.message()),
            ProjectNote::File { err, subject } => write!(l, "{}: {subject}", err.message()),
            ProjectNote::Missing(s) => write!(l, "PROJECT NOT FOUND: {s}"),
            ProjectNote::LoadFailed(s) => write!(l, "LOAD FAILED: {s}"),
            ProjectNote::Changed => l.write_str("CHANGED SINCE ASKED: TRY AGAIN"),
            ProjectNote::IsLoaded => l.write_str("CAN NOT DELETE THE LOADED PROJECT"),
            ProjectNote::NoIds => l.write_str("NO PROJECT IDS LEFT"),
            ProjectNote::NewProject => l.write_str("NEW PROJECT"),
        };
        l
    }
}
