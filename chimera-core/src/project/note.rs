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

/// `msg: subject`. Past `LINE_LEN` the message gives way, so the name or
/// file always shows whole: it drops its last `: ` clause, else its last
/// words, never half a word.
fn naming(msg: &str, subject: Subject) -> Line {
    let mut sub = Line::new("");
    let _ = write!(sub, "{subject}");
    let room = LINE_LEN.saturating_sub(sub.len as usize + 2);
    let msg = if msg.len() <= room {
        msg
    } else {
        let mut cut = room;
        while !msg.is_char_boundary(cut) {
            cut -= 1;
        }
        let fits = &msg[..cut];
        let word_end = msg[cut..].starts_with(' ');
        let at = fits
            .rfind(": ")
            .or_else(|| if word_end { Some(cut) } else { fits.rfind(' ') })
            .unwrap_or(cut);
        &msg[..at]
    };
    let mut l = Line::new(msg.trim_end());
    let _ = write!(l, ": {}", sub.as_str());
    l
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
            } => return naming(err.message(), s),
            ProjectNote::File { err, subject } => return naming(err.message(), subject),
            ProjectNote::Missing(s) => return naming("PROJECT NOT FOUND", s),
            ProjectNote::LoadFailed(s) => return naming("LOAD FAILED", s),
            ProjectNote::Changed => l.write_str("CHANGED SINCE ASKED: TRY AGAIN"),
            ProjectNote::IsLoaded => l.write_str("CAN NOT DELETE THE LOADED PROJECT"),
            ProjectNote::NoIds => l.write_str("NO PROJECT IDS LEFT"),
            ProjectNote::NewProject => l.write_str("NEW PROJECT"),
        };
        l
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With no `: ` clause to drop, the message gives way at a word.
    #[test]
    fn a_long_message_is_cut_at_a_word() {
        let f = Subject::File(ProjectId::new(7).unwrap());
        let msg = "THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG TWICE";
        let l = naming(msg, f);
        assert_eq!(l.as_str(), "THE QUICK BROWN FOX JUMPS OVER THE: P0000007");
        assert_eq!(
            naming("ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMNOPQRSTUVWXYZ", f)
                .as_str()
                .len(),
            LINE_LEN
        );
    }
}
