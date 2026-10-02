//! Projects on the card (projects spec § Storage, ADR 0046):
//! `/CHIMERA/PROJECTS/P#######.A` and `.B`, saved from live state, loaded
//! in place, listed by header. Each call is one `Card::run`; every error
//! comes back as a `ProjectNote` for the screen.

use core::ops::ControlFlow;

use chimera_hal::store::{Dir, FileName, ReadSink, Store, StoreError, VolumeId};

use crate::name::ProjectName;
use crate::storage::{
    AbFile, Card, CardEvent, CardFault, FileError, FileKind, Generation, HEADER_LEN, Header,
    InPlaceError, LoadError, Outcome, ProjectId, Ready, RecordWriter, SaveError, Side, delete_ab,
    load_ab_in_place, peek_header, save_ab,
};

use super::codec::encode_new_project;
use super::note::{Differ, ProjectNote, Subject};
use super::{
    Confirmed, DeleteTarget, LoadLink, NEW_PROJECT_NAME, Origin, OverwriteTarget, PartId,
    PartStatus, Project, ProjectCheck, ProjectDecoder, ProjectFile, ProjectSource, Seen, Swap,
    encode_project, part_status, project_crc,
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

/// A file no pair had when `new_project_id` read the card. Nothing else
/// makes one, and a save takes it by value:
///
/// ```compile_fail,E0423
/// use chimera_core::project::{FreshFile, ProjectFile};
/// fn forge(f: ProjectFile) -> FreshFile {
///     FreshFile(f)
/// }
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct FreshFile(ProjectFile);

impl FreshFile {
    pub fn file(&self) -> ProjectFile {
        self.0
    }

    /// For tests that save to an id by hand; the save still refuses a
    /// pair already there.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn for_test(f: ProjectFile) -> Self {
        FreshFile(f)
    }
}

/// Where a save goes.
#[derive(Debug)]
pub enum SaveTo {
    /// SAVE: the project's own file.
    Own,
    /// A first save or SAVE AS.
    Fresh(FreshFile),
    /// SAVE OVER another file, as listed.
    Over(Confirmed<OverwriteTarget>),
}

/// A card operation's result, and what its mount found (`None` when it
/// failed or nothing mounted): the caller drops what was another card's.
#[must_use]
#[derive(Debug, PartialEq)]
pub struct CardOut<T> {
    pub out: T,
    pub event: Option<CardEvent>,
}

impl<T> CardOut<T> {
    fn none(out: T) -> Self {
        CardOut { out, event: None }
    }
}

/// The event of a `Card::run`, and its result flattened.
fn split<R, E>(run: Result<Outcome<R, E>, E>) -> (Result<R, E>, Option<CardEvent>) {
    match run {
        Ok(o) => (o.result, Some(o.event)),
        Err(e) => (Err(e), None),
    }
}

/// A new file on the card in the slot: the highest id there + 1; 1 with
/// no `PROJECTS` directory.
pub fn new_project_id<S: Store>(
    card: &mut Card,
    store: &mut S,
) -> CardOut<Result<FreshFile, ProjectNote>> {
    let run = card.run(store, |s, r| {
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
    let (result, event) = split(run);
    let out = match result {
        Ok((top, vol)) => ProjectId::new(top + 1)
            .map(|id| FreshFile(ProjectFile::new(id, vol)))
            .ok_or(ProjectNote::NoIds),
        Err(e) => Err(card_note(e, None)),
    };
    CardOut { out, event }
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

/// What a file must still be on the card for a write to it.
#[derive(Clone, Copy)]
enum Expect {
    /// The project's own file: nothing to check.
    Own,
    /// No pair: a `FreshFile`.
    Absent,
    /// The newest header a confirmation saw.
    Newest(Seen),
}

/// `Err` with the file's subject when it moved from `e`.
fn still<S: Store>(
    s: &mut S,
    vol: VolumeId,
    id: ProjectId,
    e: Expect,
) -> Result<Result<(), Subject>, StoreError> {
    if let Expect::Own = e {
        return Ok(Ok(()));
    }
    let now = peek_entry(s, vol, id)?;
    let held = match (e, now) {
        (Expect::Absent, None) => true,
        (Expect::Newest(seen), Some(n)) => Seen::of(&n) == seen,
        _ => false,
    };
    let subject = now
        .and_then(|n| n.name)
        .map_or(Subject::File(id), Subject::Name);
    Ok(if held { Ok(()) } else { Err(subject) })
}

/// `body` to `file`'s write side, named `name`, making the directories
/// first; only on `file.vol` and only if it still is as `e` says. A card
/// or file error names `subject`.
fn write_project<S: Store>(
    card: &mut Card,
    store: &mut S,
    (file, e): (ProjectFile, Expect),
    name: ProjectName,
    subject: Subject,
    body: &mut dyn FnMut(&mut RecordWriter<'_>) -> Result<(), StoreError>,
) -> CardOut<Result<(), ProjectNote>> {
    let run = run_on(card, store, file.vol(), |s, r| {
        if let Err(moved) = still(s, r.volume(), file.id(), e)? {
            return Ok(Err(moved));
        }
        s.make_dir(r.volume(), Dir::Chimera)?;
        s.make_dir(r.volume(), Dir::Projects)?;
        save_ab(
            s,
            r,
            project_file(file.id()),
            &mut ProjectCheck::new(),
            Some(name),
            body,
        )
        .map(Ok)
    });
    let (result, event) = split(run);
    let out = match result {
        Ok(Ok(Ok(_))) => Ok(()),
        Ok(Ok(Err(moved))) => Err(ProjectNote::FileChanged(moved)),
        Ok(Err(now)) => Err(changed(now, subject)),
        Err(SaveError::Store(err)) => Err(card_note(err, Some(subject))),
        Err(SaveError::File(err)) => Err(ProjectNote::File { err, subject }),
    };
    CardOut { out, event }
}

/// Streams `p` to its pair, on that file's card only: another card is
/// `CARD CHANGED`, and a fresh or confirmed file that moved since is
/// `FileChanged`, with nothing written. On success the project is
/// `Saved` as that file; on any error its meta is untouched.
pub fn save_project<S: Store>(
    card: &mut Card,
    store: &mut S,
    p: &mut Project,
    to: SaveTo,
) -> CardOut<ProjectNote> {
    let name = p.meta.name;
    save_project_as(card, store, p, to, name)
}

/// `save_project` under `name`: the project takes it only once saved, so
/// a refused save leaves it as it was.
pub fn save_project_as<S: Store>(
    card: &mut Card,
    store: &mut S,
    p: &mut Project,
    to: SaveTo,
    name: ProjectName,
) -> CardOut<ProjectNote> {
    let (file, e) = match to {
        SaveTo::Own => match p.meta.file {
            Some(f) => (f, Expect::Own),
            None => return CardOut::none(ProjectNote::NoFile),
        },
        SaveTo::Fresh(f) => (f.0, Expect::Absent),
        SaveTo::Over(c) => (c.target().file(), Expect::Newest(c.witness())),
    };
    let live = &*p;
    let out = write_project(
        card,
        store,
        (file, e),
        name,
        Subject::Name(name),
        &mut |w| encode_project(live, w),
    );
    let note = match out.out {
        Ok(()) => {
            p.bump();
            p.meta.name = name;
            p.meta.file = Some(file);
            p.meta.saved_crc = Some(project_crc(p));
            ProjectNote::Saved(differ(p))
        }
        Err(n) => n,
    };
    CardOut {
        out: note,
        event: out.event,
    }
}

/// CLEAR: the confirmed file becomes NEW, streamed by
/// `encode_new_project`, on its card only; `FileChanged` if it moved
/// since it was listed. The loaded project's own file is refused
/// (`ClearLoaded`), as RAM would still read `Saved`: clearing your own is
/// a guarded load of NEW, then `SaveTo::Over`.
pub fn clear_project<S: Store>(
    card: &mut Card,
    store: &mut S,
    loaded: &Project,
    c: Confirmed<OverwriteTarget>,
) -> CardOut<Result<(), ProjectNote>> {
    let file = c.target().file();
    if loaded.meta().file() == Some(file) {
        return CardOut::none(Err(ProjectNote::ClearLoaded));
    }
    write_project(
        card,
        store,
        (file, Expect::Newest(c.witness())),
        NEW_PROJECT_NAME,
        Subject::File(file.id()),
        &mut |w| encode_new_project(w),
    )
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
            p.bump();
            p.meta.file = Some(ProjectFile::new(id, vol));
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
        // SYSTEM's defaults name no project with no card: mount to say why.
        None => match card.run(store, |_, _| Ok::<_, StoreError>(())) {
            Ok(_) => ProjectNote::NewProject,
            Err(e) => card_note(e, None),
        },
        Some(id) => {
            let subject = Subject::File(id);
            let run = card.run(store, |s, r| {
                load_ab_in_place(s, r, project_file(id), &mut ProjectDecoder::new(p))
                    .map(|_| r.volume())
            });
            match run {
                Ok(o) => match o.result {
                    Ok(vol) => {
                        p.bump();
                        p.meta.file = Some(ProjectFile::new(id, vol));
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
    /// The newer readable side's: what a confirmation of it holds.
    pub generation: Option<Generation>,
}

impl ProjectEntry {
    /// The file to load or delete: on the card it was listed on.
    pub fn file(&self) -> ProjectFile {
        ProjectFile::new(self.id, self.vol)
    }
}

/// The most entries one listing holds (SETTINGS' LOAD and MANAGE).
pub const MAX_LISTED: usize = 48;

/// `list_projects`' result: `event` when the card mounted; `more` when
/// ids past the `MAX_LISTED` lowest were left out; `fresh`, the id a save
/// as would take, from the same pass (`None` with no note: no ids left).
#[must_use]
#[derive(Debug, PartialEq)]
pub struct ListOutcome {
    pub event: Option<CardEvent>,
    pub note: Option<ProjectNote>,
    pub more: bool,
    pub fresh: Option<FreshFile>,
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
        generation: newest.map(|h| h.generation),
        err: nnf.or(match newest {
            Some(_) => None,
            None => errs.clone().next(),
        }),
    }
}

/// `id`'s entry from its headers; `None` when neither side exists.
fn peek_entry<S: Store>(
    s: &mut S,
    vol: VolumeId,
    id: ProjectId,
) -> Result<Option<ProjectEntry>, StoreError> {
    let file = project_file(id);
    let sides = [
        peek_side(s, vol, file.side(Side::A))?,
        peek_side(s, vol, file.side(Side::B))?,
    ];
    Ok((sides != [None, None]).then(|| entry(id, vol, sides)))
}

/// What one directory pass found: the lowest ids, sorted, and the top.
struct Ids {
    low: [ProjectId; MAX_LISTED],
    len: usize,
    more: bool,
    top: u32,
}

impl Ids {
    fn see(&mut self, id: ProjectId) {
        self.top = self.top.max(id.get());
        let at = match self.low[..self.len].binary_search(&id) {
            Ok(_) => return,
            Err(at) => at,
        };
        if at == MAX_LISTED {
            self.more = true;
            return;
        }
        if self.len == MAX_LISTED {
            self.more = true;
        } else {
            self.len += 1;
        }
        self.low.copy_within(at..self.len - 1, at + 1);
        self.low[at] = id;
    }
}

/// One pass over `PROJECTS` for the ids past `after`: an empty card has no
/// directory.
fn scan<S: Store>(s: &mut S, vol: VolumeId, after: u32) -> Result<Ids, StoreError> {
    let mut ids = Ids {
        low: [ProjectId::MIN; MAX_LISTED],
        len: 0,
        more: false,
        top: 0,
    };
    match s.list(vol, Dir::Projects, &mut |name, _| {
        if let Some(id) = file_id(name)
            && id.get() > after
        {
            ids.see(id);
        }
    }) {
        Ok(()) | Err(StoreError::NotFound) => Ok(ids),
        Err(e) => Err(e),
    }
}

/// The `MAX_LISTED` lowest projects on the card, in id order, from their
/// headers alone (one block a side), after one directory pass.
pub fn list_projects<S: Store>(
    card: &mut Card,
    store: &mut S,
    f: &mut dyn FnMut(ProjectEntry),
) -> ListOutcome {
    let run = card.run(store, |s, r| {
        let vol = r.volume();
        let ids = scan(s, vol, 0)?;
        for &id in &ids.low[..ids.len] {
            if let Some(e) = peek_entry(s, vol, id)? {
                f(e);
            }
        }
        Ok((
            ids.more,
            ProjectId::new(ids.top + 1).map(|id| FreshFile(ProjectFile::new(id, vol))),
        ))
    });
    match run {
        Ok(Outcome {
            event,
            result: Ok((more, fresh)),
        }) => ListOutcome {
            event: Some(event),
            note: None,
            more,
            fresh,
        },
        Ok(Outcome {
            event,
            result: Err(e),
        }) => ListOutcome {
            event: Some(event),
            note: Some(card_note(e, None)),
            more: false,
            fresh: None,
        },
        Err(e) => ListOutcome {
            event: None,
            note: Some(card_note(e, None)),
            more: false,
            fresh: None,
        },
    }
}

/// The lowest id past `after` whose name is `n`, ignoring case: one
/// directory pass per `MAX_LISTED` ids, stopping at the first match.
pub fn find_named<S: Store>(
    card: &mut Card,
    store: &mut S,
    n: &ProjectName,
    after: u32,
) -> CardOut<Result<Option<ProjectEntry>, ProjectNote>> {
    let run = card.run(store, |s, r| {
        let vol = r.volume();
        let mut after = after;
        loop {
            let ids = scan(s, vol, after)?;
            for &id in &ids.low[..ids.len] {
                if let Some(e) = peek_entry(s, vol, id)?
                    && e.name
                        .is_some_and(|m| m.as_str().eq_ignore_ascii_case(n.as_str()))
                {
                    return Ok(Some(e));
                }
            }
            match ids.low[..ids.len].last() {
                Some(last) if ids.more => after = last.get(),
                _ => return Ok(None),
            }
        }
    });
    let (result, event) = split(run);
    CardOut {
        out: result.map_err(|e| card_note(e, None)),
        event,
    }
}

/// Deletes the confirmed file's pair, the side a load wouldn't keep
/// first, on its card only (another card is `CARD CHANGED`). The loaded
/// project's own file is refused, and so is a file that moved since it
/// was listed (`FileChanged`, gone included): nothing is deleted.
pub fn delete_project<S: Store>(
    card: &mut Card,
    store: &mut S,
    loaded: &Project,
    c: Confirmed<DeleteTarget>,
) -> CardOut<Result<(), ProjectNote>> {
    let file = c.target().file();
    if loaded.meta().file() == Some(file) {
        return CardOut::none(Err(ProjectNote::IsLoaded));
    }
    let run = run_on(card, store, file.vol(), |s, r| {
        if let Err(moved) = still(s, r.volume(), file.id(), Expect::Newest(c.witness()))? {
            return Ok(Err(moved));
        }
        delete_ab(s, r, project_file(file.id()), &mut ProjectCheck::new()).map(Ok)
    });
    let subject = Subject::File(file.id());
    let (result, event) = split(run);
    let out = match result {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(moved))) => Err(ProjectNote::FileChanged(moved)),
        Ok(Err(now)) => Err(changed(now, subject)),
        Err(e) => Err(card_note(e, Some(subject))),
    };
    CardOut { out, event }
}
