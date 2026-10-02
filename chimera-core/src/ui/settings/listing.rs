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

/// The most entries held; a card with more ends the list with `MORE ON CARD`.
pub const MAX_LISTED: usize = 48;

/// An entry without its card: the listing holds the volume once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Listed {
    pub id: ProjectId,
    pub name: Option<ProjectName>,
    pub err: Option<FileError>,
    pub generation: Option<Generation>,
}

/// What was read: the entries are `Listing::items`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rows {
    Unread,
    /// The card couldn't be read (NO CARD): one dimmed row says why.
    Unreadable(StoreError),
    Card {
        vol: VolumeId,
        more: bool,
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
    /// On `Rows::Card` only, from the first; `None` past the last.
    items: [Option<Listed>; MAX_LISTED],
    validity: Validity,
    /// Moves on every re-list: the list band's key.
    revision: u16,
}

/// What a LOAD row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Entry(ProjectEntry),
    CreateNew,
    /// NO CARD, MORE ON CARD: nothing runs.
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
            items: [None; MAX_LISTED],
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

    /// Another card is in the slot: stale, and no entry's witness holds.
    pub fn swapped(&mut self) {
        self.mark_stale();
        for l in self.items.iter_mut().flatten() {
            l.generation = None;
        }
    }

    /// The listed entries.
    pub fn len(&self) -> usize {
        self.items.iter().take_while(|i| i.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn entry(&self, i: usize) -> Option<ProjectEntry> {
        let Rows::Card { vol, .. } = self.rows else {
            return None;
        };
        self.items.get(i).copied().flatten().map(|l| ProjectEntry {
            id: l.id,
            vol,
            name: l.name,
            err: l.err,
            generation: l.generation,
        })
    }

    /// The entry named `n`, ignoring case; the lowest id of several.
    pub fn named(&self, n: &ProjectName) -> Option<ProjectEntry> {
        let n = n.as_str();
        (0..self.len())
            .filter_map(|i| self.entry(i))
            .find(|e| e.name.is_some_and(|m| m.as_str().eq_ignore_ascii_case(n)))
    }

    /// Starts a re-list: empty on `vol`.
    pub(crate) fn begin(&mut self, vol: VolumeId) {
        self.rows = Rows::Card { vol, more: false };
        self.items = [None; MAX_LISTED];
        self.validity = Validity::Current;
        self.revision = self.revision.wrapping_add(1);
    }

    pub(crate) fn push(&mut self, e: ProjectEntry) {
        let n = self.len();
        match (self.items.get_mut(n), &mut self.rows) {
            (Some(slot), Rows::Card { .. }) => {
                *slot = Some(Listed {
                    id: e.id,
                    name: e.name,
                    err: e.err,
                    generation: e.generation,
                })
            }
            (None, Rows::Card { more, .. }) => *more = true,
            _ => {}
        }
    }

    pub(crate) fn unreadable(&mut self, e: StoreError) {
        self.rows = Rows::Unreadable(e);
        self.items = [None; MAX_LISTED];
        self.validity = Validity::Current;
        self.revision = self.revision.wrapping_add(1);
    }

    fn more(&self) -> bool {
        matches!(self.rows, Rows::Card { more: true, .. })
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
            return (i == 0).then_some(Pick::Inert);
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
            Pick::Inert => match self.rows {
                Rows::Unreadable(e) => row(e.message(), None, RowLook::Dimmed),
                Rows::Unread => row("", None, RowLook::Dimmed),
                Rows::Card { .. } => row(MORE_ON_CARD, None, RowLook::Dimmed),
            },
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
