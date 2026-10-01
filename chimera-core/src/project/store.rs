//! Projects on the card (projects spec § Storage, ADR 0046):
//! `/CHIMERA/PROJECTS/P#######.A` and `.B`, saved from live state, loaded
//! in place, listed by header. Each call is one `Card::run`; every error
//! comes back as a `ProjectNote` for the screen.

use core::ops::ControlFlow;

use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError, VolumeId};

use crate::name::ProjectName;
use crate::storage::{
    AbFile, Card, CardEvent, CardFault, FileError, FileKind, HEADER_LEN, Header, InPlaceError,
    LoadError, Outcome, ProjectId, Ready, SaveError, Side, delete_ab, load_ab_in_place,
    peek_header, save_ab,
};

use super::note::{Differ, ProjectNote, Subject};
use super::{
    Confirmed, LoadLink, Origin, PartId, PartStatus, Project, ProjectCheck, ProjectDecoder,
    ProjectFile, ProjectSource, Swap, encode_project, part_status, project_crc,
};

pub fn project_file(id: ProjectId) -> AbFile {
    AbFile::new(Dir::Projects, &id.stem()).expect("P + 7 digits is a stem")
}

/// A `P#######.A` or `.B` name's id.
fn file_id(f: FileName) -> Option<ProjectId> {
    let (&[b'P', ref digits @ ..], b"A" | b"B") = (f.stem(), f.ext()) else {
        return None;
    };
    if digits.len() != 7 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n = digits.iter().fold(0, |n, d| n * 10 + u32::from(d - b'0'));
    ProjectId::new(n)
}

/// `op` on the card `vol` only. Another card in the slot is `Ok(Err(now))`:
/// nothing is read or written, and the healthy card isn't marked failed.
fn run_on<S: Store, R, E: CardFault + From<StoreError>>(
    card: &mut Card,
    store: &mut S,
    vol: VolumeId,
    op: impl FnOnce(&mut S, &Ready) -> Result<R, E>,
) -> Result<Outcome<Result<R, VolumeId>, E>, E> {
    card.run(store, |s, r| {
        if r.volume() != vol {
            return Ok(Err(r.volume()));
        }
        op(s, r).map(Ok)
    })
}

fn changed(now: VolumeId, subject: Subject) -> ProjectNote {
    card_note(StoreError::VolumeChanged(now), Some(subject))
}

fn card_note(err: StoreError, subject: Option<Subject>) -> ProjectNote {
    ProjectNote::Card { err, subject }
}

fn load_note(e: LoadError, subject: Subject) -> ProjectNote {
    match e {
        LoadError::Store(err) => card_note(err, Some(subject)),
        LoadError::File(err) => ProjectNote::File { err, subject },
        LoadError::Missing => ProjectNote::Missing(subject),
    }
}

/// A new file on the card in the slot: the highest id there + 1; 1 with
/// no `PROJECTS` directory.
pub fn new_project_id<S: Store>(
    card: &mut Card,
    store: &mut S,
) -> Result<ProjectFile, ProjectNote> {
    let out = card.run(store, |s, r| {
        let vol = r.volume();
        let mut top = 0;
        match s.list(r.volume(), Dir::Projects, &mut |f, _| {
            if let Some(id) = file_id(f) {
                top = top.max(id.get());
            }
        }) {
            Ok(()) | Err(StoreError::NotFound) => Ok((top, vol)),
            Err(e) => Err(e),
        }
    });
    match out.and_then(|o| o.result) {
        Ok((top, vol)) => ProjectId::new(top + 1)
            .map(|id| ProjectFile { id, vol })
            .ok_or(ProjectNote::NoIds),
        Err(e) => Err(card_note(e, None)),
    }
}

/// The Parts saved `Edited` from a slot.
fn differ(p: &Project) -> Differ {
    let mut edited = PartId::ALL.into_iter().filter_map(|id| {
        let part = p.part(id);
        match part.origin {
            Origin::Slot { slot, .. } if part_status(part, &p.pool) == PartStatus::Edited => {
                Some((id, slot))
            }
            _ => None,
        }
    });
    match (edited.next(), edited.count()) {
        (None, _) => Differ::None,
        (Some((id, slot)), 0) => Differ::One(id, slot),
        (Some(_), rest) => Differ::Many(rest as u8 + 1),
    }
}

/// Streams `p` to `to`'s pair, making the directories first: SAVE passes
/// `meta().file()`, a first save or SAVE AS `new_project_id`'s. Only on
/// `to.vol`: another card is `CARD CHANGED` with nothing written. On
/// success the project is `Saved` as `to`; on any error its meta is
/// untouched.
pub fn save_project<S: Store>(
    card: &mut Card,
    store: &mut S,
    p: &mut Project,
    to: ProjectFile,
) -> ProjectNote {
    let name = p.meta.name;
    let subject = Subject::Name(name);
    let live = &*p;
    let id = to.id;
    let out = run_on(card, store, to.vol, |s, r| {
        s.make_dir(r.volume(), Dir::Chimera)?;
        s.make_dir(r.volume(), Dir::Projects)?;
        save_ab(
            s,
            r,
            project_file(id),
            &mut ProjectCheck::new(),
            Some(name),
            &mut |w| encode_project(live, w),
        )
    });
    match out.and_then(|o| o.result) {
        Ok(Ok(_)) => {
            p.meta.file = Some(to);
            p.meta.saved_crc = Some(project_crc(p));
            ProjectNote::Saved(differ(p))
        }
        Ok(Err(now)) => changed(now, subject),
        Err(SaveError::Store(err)) => card_note(err, Some(subject)),
        Err(SaveError::File(err)) => ProjectNote::File { err, subject },
    }
}

/// A load's result: `swap` when the project was replaced (loaded, or
/// reset to NEW), which the caller settles and publishes; `note` for the
/// screen; `event` when the card mounted.
///
/// A file loaded and `+ NEW` look alike here (a swap, no note). What the
/// caller does only for a file (SYSTEM's last project) keys on the
/// confirmed source, or on `meta().file()` being set.
#[must_use]
#[derive(Debug)]
pub struct LoadOutcome {
    pub swap: Option<Swap>,
    pub note: Option<ProjectNote>,
    pub event: Option<CardEvent>,
}

/// Replaces `p` as `go` confirmed: NEW, or a file in place (ADR 0046).
/// The only writer of the load epoch. Refused, with `Changed`, if `p`
/// moved since `go`; a file listed on another card is refused before any
/// read. A pass 1 failure leaves `p` untouched; a pass 2 failure resets it
/// to NEW, and still swaps.
pub fn load_project<S: Store>(
    card: &mut Card,
    store: &mut S,
    p: &mut Project,
    go: Confirmed<ProjectSource>,
    link: &LoadLink,
) -> LoadOutcome {
    let mut out = LoadOutcome {
        swap: None,
        note: None,
        event: None,
    };
    if !go.holds(p) {
        out.note = Some(ProjectNote::Changed);
        return out;
    }
    let (id, vol) = match go.target() {
        ProjectSource::New => {
            p.reset_new();
            out.swap = Some(link.bump());
            return out;
        }
        ProjectSource::File { id, vol } => (id, vol),
    };
    let subject = Subject::File(id);
    let run = run_on(card, store, vol, |s, r| {
        load_ab_in_place(s, r, project_file(id), &mut ProjectDecoder::new(p))
    });
    let result = match run {
        Ok(o) => {
            out.event = Some(o.event);
            o.result
        }
        Err(e) => Err(e),
    };
    match result {
        Ok(Ok(_)) => {
            p.meta.file = Some(ProjectFile { id, vol });
            p.meta.saved_crc = Some(project_crc(p));
            out.swap = Some(link.bump());
        }
        // Not the card the file was listed on: nothing read.
        Ok(Err(now)) => out.note = Some(changed(now, subject)),
        Err(InPlaceError {
            clobbered: false,
            err,
        }) => out.note = Some(load_note(err, subject)),
        Err(InPlaceError {
            clobbered: true, ..
        }) => {
            p.reset_new();
            out.swap = Some(link.bump());
            out.note = Some(ProjectNote::LoadFailed(subject));
        }
    }
    out
}

/// Boot's load of SYSTEM's last project (spec § Boot). Nothing sounds
/// yet, so there is no epoch to bump (ADR 0046). A load binds the project
/// to the card it came from; anything else leaves `p` NEW, whatever it
/// held, and says why.
pub fn boot_project<S: Store>(
    card: &mut Card,
    store: &mut S,
    last: Option<ProjectId>,
    p: &mut Project,
) -> Option<ProjectNote> {
    let note = match last {
        None => ProjectNote::NewProject,
        Some(id) => {
            let subject = Subject::File(id);
            let run = card.run(store, |s, r| {
                load_ab_in_place(s, r, project_file(id), &mut ProjectDecoder::new(p))
                    .map(|_| r.volume())
            });
            match run {
                Ok(o) => match o.result {
                    Ok(vol) => {
                        p.meta.file = Some(ProjectFile { id, vol });
                        p.meta.saved_crc = Some(project_crc(p));
                        return None;
                    }
                    Err(InPlaceError {
                        clobbered: true, ..
                    }) => ProjectNote::LoadFailed(subject),
                    Err(InPlaceError { err, .. }) => load_note(err, subject),
                },
                // The mount failed, so no file was reached. `Card::run`
                // builds that error from the mount's `StoreError`.
                Err(e) => card_note(e.store_error().unwrap_or(StoreError::Io), None),
            }
        }
    };
    p.reset_new();
    Some(note)
}

/// A project as its headers list it, on the card `vol`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectEntry {
    pub id: ProjectId,
    pub vol: VolumeId,
    /// The newer readable side's.
    pub name: Option<ProjectName>,
    /// What a load would refuse it for, when the headers already say.
    pub err: Option<FileError>,
}

impl ProjectEntry {
    /// The file to load or delete: on the card it was listed on.
    pub fn file(&self) -> ProjectFile {
        ProjectFile {
            id: self.id,
            vol: self.vol,
        }
    }
}

/// `list_projects`' result: `event` when the card mounted.
#[must_use]
#[derive(Debug, PartialEq)]
pub struct ListOutcome {
    pub event: Option<CardEvent>,
    pub note: Option<ProjectNote>,
}

/// The first `HEADER_LEN` bytes of a file, then a break.
struct HeadSink {
    buf: [u8; HEADER_LEN],
    len: usize,
}

impl ReadSink for HeadSink {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        if (len as usize) < HEADER_LEN {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        let n = bytes.len().min(HEADER_LEN - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
        if self.len == HEADER_LEN {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// One side's header: `None` when it doesn't exist.
fn peek_side<S: Store>(
    s: &mut S,
    vol: VolumeId,
    f: FileName,
) -> Result<Option<Result<Header, FileError>>, StoreError> {
    let mut sink = HeadSink {
        buf: [0; HEADER_LEN],
        len: 0,
    };
    match s.read(vol, f, &mut sink) {
        Ok(()) => {}
        Err(StoreError::NotFound) => return Ok(None),
        Err(StoreError::Corrupt) => return Ok(Some(Err(FileError::Truncated))),
        Err(e) => return Err(e),
    }
    if sink.len < HEADER_LEN {
        return Ok(Some(Err(FileError::Truncated)));
    }
    Ok(Some(peek_header(&sink.buf).and_then(|h| {
        if h.kind == FileKind::Project {
            Ok(h)
        } else {
            Err(FileError::WrongKind)
        }
    })))
}

/// An id's entry from its two headers. A side that needs newer firmware
/// is the entry's error even beside a readable one, as `pick` refuses it
/// first; else the newer readable side's name; else the first error.
fn entry(
    id: ProjectId,
    vol: VolumeId,
    sides: [Option<Result<Header, FileError>>; 2],
) -> ProjectEntry {
    let newest = sides
        .iter()
        .filter_map(|s| s.and_then(Result::ok))
        .reduce(|a, b| {
            if b.generation.is_newer_than(a.generation) {
                b
            } else {
                a
            }
        });
    let errs = sides.iter().filter_map(|s| s.and_then(Result::err));
    let nnf = errs.clone().find(|&e| e == FileError::NeedsNewerFirmware);
    ProjectEntry {
        id,
        vol,
        name: newest.and_then(|h| h.name),
        err: nnf.or(match newest {
            Some(_) => None,
            None => errs.clone().next(),
        }),
    }
}

/// Each project on the card, in id order, from its headers alone (one
/// block a side). Each id costs a directory scan to find: O(n²) sectors.
pub fn list_projects<S: Store>(
    card: &mut Card,
    store: &mut S,
    f: &mut dyn FnMut(ProjectEntry),
) -> ListOutcome {
    let run = card.run(store, |s, r| {
        let vol = r.volume();
        let mut after = 0;
        loop {
            let mut next: Option<ProjectId> = None;
            match s.list(vol, Dir::Projects, &mut |name, _| {
                if let Some(id) = file_id(name)
                    && id.get() > after
                    && next.is_none_or(|n| id < n)
                {
                    next = Some(id);
                }
            }) {
                Ok(()) => {}
                Err(StoreError::NotFound) => return Ok(()),
                Err(e) => return Err(e),
            }
            let Some(id) = next else { return Ok(()) };
            after = id.get();
            let file = project_file(id);
            let sides = [
                peek_side(s, vol, file.side(Side::A))?,
                peek_side(s, vol, file.side(Side::B))?,
            ];
            if sides != [None, None] {
                f(entry(id, vol, sides));
            }
        }
    });
    match run {
        Ok(o) => ListOutcome {
            event: Some(o.event),
            note: o.result.err().map(|e| card_note(e, None)),
        },
        Err(e) => ListOutcome {
            event: None,
            note: Some(card_note(e, None)),
        },
    }
}

/// Deletes `file`'s pair, the side a load wouldn't keep first, on
/// `file.vol` only (another card is `CARD CHANGED`). The loaded project's
/// own file is refused. A pair already gone is Ok.
pub fn delete_project<S: Store>(
    card: &mut Card,
    store: &mut S,
    loaded: &Project,
    file: ProjectFile,
) -> Result<(), ProjectNote> {
    if loaded.meta().file() == Some(file) {
        return Err(ProjectNote::IsLoaded);
    }
    let subject = Subject::File(file.id);
    let out = run_on(card, store, file.vol, |s, r| {
        delete_ab(s, r, project_file(file.id), &mut ProjectCheck::new())
    });
    match out.and_then(|o| o.result) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(now)) => Err(changed(now, subject)),
        Err(e) => Err(card_note(e, Some(subject))),
    }
}
