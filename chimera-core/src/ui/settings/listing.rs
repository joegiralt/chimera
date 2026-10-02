//! The card's projects as LOAD PROJECT and MANAGE PROJECTS list them: read
//! by `card_work`, never by a frame. A row resolves to a `Pick` holding the
//! entry by value, so nothing keeps a row index past a re-list but the
//! bar, which the re-list clamps.

use core::fmt::Write;

use chimera_hal::store::{StoreError, VolumeId};

use crate::name::ProjectName;
use crate::project::{Line, ProjectEntry, ProjectFile, ProjectNote, Subject};
use crate::storage::{FileError, Generation, ProjectId};

use super::view::RowLook;

pub use crate::project::MAX_LISTED;

/// What a file's headers said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    /// The newer readable side; `newer_fw` when a side needs newer
    /// firmware, which a load refuses first.
    Read {
        generation: Generation,
        name: Option<ProjectName>,
        newer_fw: bool,
    },
    /// No side readable.
    Bad(FileError),
}

/// An entry without its card: the listing holds the volume once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Listed {
    pub id: ProjectId,
    pub head: Head,
}

impl Listed {
    fn of(e: &ProjectEntry) -> Self {
        let head = match e.generation {
            Some(generation) => Head::Read {
                generation,
                name: e.name,
                newer_fw: e.err == Some(FileError::NeedsNewerFirmware),
            },
            None => Head::Bad(e.err.unwrap_or(FileError::Corrupt)),
        };
        Listed { id: e.id, head }
    }

    fn entry(self, vol: VolumeId) -> ProjectEntry {
        let (name, err, generation) = match self.head {
            Head::Read {
                generation,
                name,
                newer_fw,
            } => (
                name,
                newer_fw.then_some(FileError::NeedsNewerFirmware),
                Some(generation),
            ),
            Head::Bad(e) => (None, Some(e), None),
        };
        ProjectEntry {
            id: self.id,
            vol,
            name,
            err,
            generation,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(clippy::large_enum_variant, reason = "one static")]
enum Rows {
    Unread,
    /// The card couldn't be read (NO CARD): one dimmed row says why.
    Unreadable(StoreError),
    /// `items[..len]` in id order; `more` past them on the card.
    Card {
        vol: VolumeId,
        more: bool,
        items: [Listed; MAX_LISTED],
        len: u8,
    },
}

/// Whether the rows are the card as last read, or something since moved it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Validity {
    Current,
    Stale,
}

pub struct Listing {
    rows: Rows,
    validity: Validity,
    /// Moves on every re-list: the list band's key.
    revision: u16,
}

/// What a LOAD row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Entry(ProjectEntry),
    CreateNew,
    /// NO CARD and the like: SEQ reads the card again.
    Retry,
    /// MORE ON CARD: nothing runs.
    Inert,
}

/// A LOAD row as drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadRow {
    pub label: Line,
    pub note: Option<&'static str>,
    pub look: RowLook,
}

pub const LOADED: &str = "● LOADED";
pub const DAMAGED: &str = "FILE DAMAGED";
pub const NEWER: &str = "NEWER FIRMWARE";
const CREATE_NEW: &str = "+ CREATE NEW";
const MORE_ON_CARD: &str = "MORE ON CARD";

impl Default for Listing {
    fn default() -> Self {
        Self::new()
    }
}

impl Listing {
    pub const fn new() -> Self {
        Listing {
            rows: Rows::Unread,
            validity: Validity::Stale,
            revision: 0,
        }
    }

    pub fn validity(&self) -> Validity {
        self.validity
    }

    pub fn revision(&self) -> u16 {
        self.revision
    }

    pub fn mark_stale(&mut self) {
        self.validity = Validity::Stale;
    }

    /// Another card is in the slot: the rows, and every witness in them,
    /// go; the next re-list reads the new card.
    pub fn swapped(&mut self) {
        self.rows = Rows::Unread;
        self.mark_stale();
    }

    fn items(&self) -> &[Listed] {
        match &self.rows {
            Rows::Card { items, len, .. } => &items[..*len as usize],
            _ => &[],
        }
    }

    /// The listed entries.
    pub fn len(&self) -> usize {
        self.items().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The card the entries are on.
    pub fn vol(&self) -> Option<VolumeId> {
        match self.rows {
            Rows::Card { vol, .. } => Some(vol),
            _ => None,
        }
    }

    /// Ids past the listed ones are on the card.
    pub fn more(&self) -> bool {
        matches!(self.rows, Rows::Card { more: true, .. })
    }

    pub fn entry(&self, i: usize) -> Option<ProjectEntry> {
        let vol = self.vol()?;
        self.items().get(i).map(|l| l.entry(vol))
    }

    /// The listed entry named `n`, ignoring case; the lowest id of several.
    pub fn named(&self, n: &ProjectName) -> Option<ProjectEntry> {
        let n = n.as_str();
        (0..self.len())
            .filter_map(|i| self.entry(i))
            .find(|e| e.name.is_some_and(|m| m.as_str().eq_ignore_ascii_case(n)))
    }

    /// Starts a re-list: empty on `vol`.
    pub(crate) fn begin(&mut self, vol: VolumeId) {
        self.rows = Rows::Card {
            vol,
            more: false,
            items: [Listed {
                id: ProjectId::MIN,
                head: Head::Bad(FileError::Corrupt),
            }; MAX_LISTED],
            len: 0,
        };
        self.validity = Validity::Current;
        self.revision = self.revision.wrapping_add(1);
    }

    /// The next entry; past `MAX_LISTED`, only `more`.
    pub(crate) fn push(&mut self, e: ProjectEntry) {
        if let Rows::Card {
            items, len, more, ..
        } = &mut self.rows
        {
            match items.get_mut(*len as usize) {
                Some(slot) => {
                    *slot = Listed::of(&e);
                    *len += 1;
                }
                None => *more = true,
            }
        }
    }

    /// The re-list's end: whether the card has more.
    pub(crate) fn ended(&mut self, has_more: bool) {
        if let Rows::Card { more, .. } = &mut self.rows {
            *more |= has_more;
        }
    }

    pub(crate) fn unreadable(&mut self, e: StoreError) {
        self.rows = Rows::Unreadable(e);
        self.validity = Validity::Current;
        self.revision = self.revision.wrapping_add(1);
    }

    /// LOAD PROJECT's rows: the entries, `MORE ON CARD`, then `+ CREATE
    /// NEW`; with no card, just the reason.
    pub fn load_rows(&self) -> usize {
        match self.rows {
            Rows::Card { .. } => self.len() + self.more() as usize + 1,
            Rows::Unreadable(_) | Rows::Unread => 1,
        }
    }

    pub fn load_pick(&self, i: usize) -> Option<Pick> {
        if let Rows::Unreadable(_) | Rows::Unread = self.rows {
            return (i == 0).then_some(Pick::Retry);
        }
        let n = self.len();
        match i.checked_sub(n) {
            None => self.entry(i).map(Pick::Entry),
            Some(0) if self.more() => Some(Pick::Inert),
            Some(k) if k == self.more() as usize => Some(Pick::CreateNew),
            Some(_) => None,
        }
    }

    /// Row `i` of LOAD PROJECT; `loaded` carries `● LOADED`.
    pub fn load_row(&self, i: usize, loaded: Option<ProjectFile>) -> Option<LoadRow> {
        let row = |s: &str, note, look| LoadRow {
            label: Line::new(s),
            note,
            look,
        };
        Some(match self.load_pick(i)? {
            Pick::Entry(e) => {
                let mut label = Line::new("");
                let _ = write!(label, "{:02}", e.id.get());
                if let Some(n) = e.name {
                    let _ = write!(
                        label,
                        " {}",
                        crate::ui::components::upper(n.as_str()).as_str()
                    );
                }
                let (note, look) = match e.err {
                    Some(FileError::NeedsNewerFirmware) => (Some(NEWER), RowLook::Dimmed),
                    Some(_) => (Some(DAMAGED), RowLook::Dimmed),
                    None if Some(e.file()) == loaded => (Some(LOADED), RowLook::Normal),
                    None => (None, RowLook::Normal),
                };
                LoadRow { label, note, look }
            }
            Pick::CreateNew => row(CREATE_NEW, None, RowLook::Normal),
            Pick::Retry => match self.rows {
                Rows::Unreadable(e) => row(e.message(), None, RowLook::Dimmed),
                _ => row("", None, RowLook::Dimmed),
            },
            Pick::Inert => row(MORE_ON_CARD, None, RowLook::Dimmed),
        })
    }
}

/// Why an errored entry won't load.
pub fn refusal(e: &ProjectEntry) -> Option<ProjectNote> {
    e.err.map(|err| ProjectNote::File {
        err,
        subject: e.name.map_or(Subject::File(e.id), Subject::Name),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(serial: u32) -> VolumeId {
        VolumeId {
            serial,
            label: *b"TEST       ",
        }
    }

    /// A swap drops every entry and its witness: nothing listed on the old
    /// card can confirm a write.
    #[test]
    fn swapped_drops_the_witnesses() {
        let mut l = Listing::new();
        l.begin(vol(1));
        l.push(ProjectEntry {
            id: ProjectId::new(3).unwrap(),
            vol: vol(1),
            name: ProjectName::new("A").ok(),
            err: None,
            generation: Some(Generation::FIRST),
        });
        assert!(l.entry(0).unwrap().generation.is_some());
        l.swapped();
        assert_eq!(l.validity(), Validity::Stale);
        assert_eq!(l.entry(0), None);
        assert!(l.named(&ProjectName::new("A").unwrap()).is_none());
        assert_eq!(l.load_pick(0), Some(Pick::Retry));
    }
}
