//! The Sound rung's LIBRARY and FACTORY tabs (library spec § Browser,
//! § Load; showcase spec § 2): the page as listed, the tab B turns, and a
//! load in flight. A pending load holds the pool's scratch and maybe its
//! staging, so while it waits no other card work starts.

use core::fmt::Write;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;

use chimera_hal::store::{Dir, Store, StoreError, VolumeId};

use crate::in_place::uninit_at;
use crate::name::SoundName;
use crate::project::{
    AdoptError, CardOut, Confirmed, LibEntry, LibNote, LibPage, LibRead, LibraryPart, Line, PageAt,
    PartId, PartSet, ReplaceError, ReplaceGuard, Shelf, SlotId, SourceNeed, Staged, list_library,
    peek_note, read_library_sound, save_library_part,
};
use crate::storage::{
    BuildStamp, Card, FactoryId, LibId, LibKind, Note, held_for_file_len, source_file,
};
use crate::ui::UiState;
use crate::ui::about_page;
use crate::ui::browser::Browse;
use crate::ui::busy::Toast;
use crate::ui::nav::{ListAt, Location};
use crate::ui::settings::job::Job;
use crate::ui::settings::prompt;
use crate::ui::settings::replace::LibraryAsk;
use crate::ui::settings::{Ask, NamingFor, PART_ROW, SAVE_TO_LIB_ROW};
use crate::ui::source_cell::{READ_FAILED, SOURCE_MISSING, SourceLoad, SourceNote, StagedStep};

/// What the Sound rung lists: PROJECT when EDIT opens it; kept across
/// MENU and naming.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserTab {
    Project,
    Library,
    /// The installed factory Sounds: read-only.
    Factory,
}

impl BrowserTab {
    pub const ALL: [BrowserTab; 3] = [
        BrowserTab::Project,
        BrowserTab::Library,
        BrowserTab::Factory,
    ];

    /// Its title, in full.
    pub const fn label(self) -> &'static str {
        match self {
            BrowserTab::Project => "PROJECT",
            BrowserTab::Library => "LIBRARY",
            BrowserTab::Factory => "FACTORY",
        }
    }

    /// The card shelf it lists; PROJECT lists the pool.
    pub const fn shelf(self) -> Option<Shelf> {
        match self {
            BrowserTab::Project => None,
            BrowserTab::Library => Some(Shelf::User),
            BrowserTab::Factory => Some(Shelf::Factory),
        }
    }

    /// `d` tabs along for B's turn, stopping at each end.
    pub fn turned(self, d: i8) -> BrowserTab {
        let i = (self as usize).saturating_add_signed(isize::from(d));
        BrowserTab::ALL[i.min(BrowserTab::ALL.len() - 1)]
    }
}

/// A library load between its read and `adopt`: the confirmation and
/// what the read said. Its source streams in the SOURCE cell's job slot
/// (`SourceCell::hold`), which it shares, as the two never stream at once.
pub(crate) struct PendingLoad {
    pub go: Confirmed<LibraryPart>,
    pub read: LibRead,
}

/// The selected FACTORY row's description, peeked on its own: the one
/// slot FACTORY keeps, and the factory Sound it belongs to.
#[derive(Clone, Copy, Debug)]
struct Described {
    id: FactoryId,
    /// `None`: that file has no description, or its peek failed.
    note: Option<Note>,
}

pub struct LibState {
    page: LibPage,
    listed: bool,
    /// The selected FACTORY row's description (showcase spec § 1, Cost).
    described: Option<Described>,
    /// Why the last listing failed: its row says so until listed again,
    /// on the next tab or rung entry.
    err: Option<LibNote>,
    pub(crate) load: Option<PendingLoad>,
    pub(crate) tab: BrowserTab,
    /// The build line 3 compares stamps with.
    pub(crate) this: BuildStamp,
    /// SAVE PART FIRST's Stale Parts and slot: asked about once the load
    /// it ran before ends, not under it.
    after: Option<(PartSet, SlotId)>,
}

// `init_in_place` writes each field: a new one fails to build here first.
crate::in_place::field_list!(LibState => LibState {
    page, listed, described, err, load, tab, this, after
});

impl LibState {
    /// PROJECT, nothing listed, written in place: the page a row at a
    /// time (`LibPage::init_in_place`), so its empty image isn't in flash.
    pub(crate) fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        let p = slot.as_mut_ptr();
        // SAFETY: `p` comes from `slot`: non-null, aligned and exclusively
        // ours. `addr_of_mut!` takes each field's place without a
        // reference; `uninit_at` views the page's place as `MaybeUninit`,
        // which never asserts it initialised, and `LibPage::init_in_place`
        // writes all of it. Every other field (`field_list!` above names
        // them all) is written exactly once with a valid value, so the
        // whole state is initialised before `assume_init_mut` makes the
        // first reference to it.
        unsafe {
            LibPage::init_in_place(uninit_at(addr_of_mut!((*p).page)));
            addr_of_mut!((*p).listed).write(false);
            addr_of_mut!((*p).described).write(None);
            addr_of_mut!((*p).err).write(None);
            addr_of_mut!((*p).load).write(None);
            addr_of_mut!((*p).tab).write(BrowserTab::Project);
            addr_of_mut!((*p).this).write(about_page::THIS);
            addr_of_mut!((*p).after).write(None);
            slot.assume_init_mut()
        }
    }

    /// A load is pending: no other card work starts.
    pub fn busy(&self) -> bool {
        self.load.is_some()
    }

    /// The page goes: listed again when LIBRARY or FACTORY is next on screen.
    pub fn forget(&mut self) {
        self.listed = false;
    }

    /// The one page static is the tab's: listed, and of its shelf.
    fn current(&self) -> bool {
        self.listed && self.tab.shelf() == Some(self.page.shelf)
    }

    pub fn tab(&self) -> BrowserTab {
        self.tab
    }

    pub fn page(&self) -> Option<&LibPage> {
        self.current().then_some(&self.page)
    }

    /// Why the page on screen is empty: the card couldn't be listed.
    pub fn err(&self) -> Option<LibNote> {
        self.err.filter(|_| self.current())
    }

    /// `e`'s description, once its peek has filled the slot: nothing while
    /// it loads.
    pub fn note_of(&self, e: &LibEntry) -> Option<Note> {
        let d = self.described?;
        (factory_id(e) == Some(d.id)).then_some(d.note)?
    }

    /// A failed listing is tried again: LIBRARY entered anew.
    pub fn entered(&mut self) {
        if self.err.is_some() {
            self.forget();
        }
    }
}

/// `e`'s id, if a factory Sound's.
fn factory_id(e: &LibEntry) -> Option<FactoryId> {
    match e.id.kind() {
        LibKind::Factory(f) => Some(f),
        LibKind::User(_) => None,
    }
}

/// A LIBRARY or FACTORY row.
#[derive(Clone, Copy, Debug)]
pub enum LibRow<'a> {
    /// `‹ NEWER` or `‹ PREVIOUS`: the page above.
    Prev,
    Sound(&'a LibEntry),
    /// `OLDER ›` or `NEXT ›`: the page below.
    Next,
    /// `NO SOUNDS YET`, `NO FACTORY SOUNDS`, or an unreadable card: EDIT
    /// does nothing.
    Empty,
}

/// How many rows the page shows: at least one.
pub fn rows(page: Option<&LibPage>) -> usize {
    let Some(p) = page else { return 1 };
    let n = usize::from(p.prev) + p.entries().len() + usize::from(p.next);
    n.max(1)
}

/// Row `i` of the page.
pub fn row(page: Option<&LibPage>, i: usize) -> LibRow<'_> {
    let Some(p) = page else {
        return LibRow::Empty;
    };
    let first = usize::from(p.prev);
    let n = p.entries().len();
    match i {
        0 if p.prev => LibRow::Prev,
        i if i >= first && i < first + n => p.entries()[i - first]
            .as_ref()
            .map_or(LibRow::Empty, LibRow::Sound),
        i if p.next && i == first + n => LibRow::Next,
        _ => LibRow::Empty,
    }
}

/// The first and last Sound rows: where a page lands the cursor.
pub fn sound_rows(page: &LibPage) -> (usize, usize) {
    let first = usize::from(page.prev);
    (first, first + page.entries().len().saturating_sub(1))
}

/// MENU while a library load streams its source.
pub const CANCELLED: &str = "CANCELLED";
pub const SKIPPED: &str = "LOADED · SOME PARAMS SKIPPED";
pub const SAVED: &str = "SAVED TO LIBRARY";

/// A library load landed in slot `s`: `LOADED TO SLOT 12`.
pub fn loaded_to(s: SlotId) -> Line {
    let mut l = Line::new("");
    let _ = write!(l, "LOADED TO SLOT {}", s.index() + 1);
    l
}

/// SETTINGS › PART, the bar on SAVE TO LIBRARY: where its NAMING opens.
const SAVE_TO_LIB_LIST: ListAt = ListAt::at(&[PART_ROW], SAVE_TO_LIB_ROW);

impl UiState {
    /// The Sound rung's tab.
    pub fn lib_tab(&self) -> BrowserTab {
        self.lib.tab
    }

    /// The library's page goes (a console `put` or `rm`): listed again
    /// when LIBRARY is next on screen.
    pub fn forget_library(&mut self) {
        self.lib.forget();
    }

    /// LIBRARY or FACTORY on screen with no page of its own: listed on the
    /// next pass.
    pub(crate) fn wants_lib_list(&self) -> bool {
        self.lib.tab.shelf().is_some()
            && !self.lib.current()
            && !self.lib.busy()
            && self.loc.browse().is_some()
    }

    /// The FACTORY row under the cursor, when its description is not the
    /// one the slot holds: peeked on the next pass, under BUSY.
    fn undescribed(&self) -> Option<(&LibEntry, FactoryId)> {
        if self.lib.tab != BrowserTab::Factory || self.lib.busy() {
            return None;
        }
        let (_, b) = self.loc.browse()?;
        let LibRow::Sound(e) = row(self.lib.page(), b.lib_cursor()) else {
            return None;
        };
        let id = factory_id(e)?;
        (self.lib.described.map(|d| d.id) != Some(id)).then_some((e, id))
    }

    pub(crate) fn wants_lib_note(&self) -> bool {
        self.undescribed().is_some()
    }

    /// The selected FACTORY row's description into the one slot. A failed
    /// peek fills it empty, so it isn't retried a pass; a card or file that
    /// moved drops the page, listed again.
    pub(crate) fn lib_peek_note<S: Store>(&mut self, card: &mut Card, store: &mut S) {
        let Some((e, id)) = self.undescribed() else {
            return;
        };
        // A page with no volume names no file: filled empty, never re-peeked.
        let Some(file) = self.lib.page.file(e) else {
            self.lib.described = Some(Described { id, note: None });
            return;
        };
        let CardOut { out, event } = peek_note(card, store, file);
        self.saw(event);
        let note = match out {
            Ok(n) => n,
            Err(n) => {
                if matches!(
                    n,
                    LibNote::Changed | LibNote::Gone | LibNote::Store(StoreError::VolumeChanged(_))
                ) {
                    self.lib.forget();
                }
                None
            }
        };
        self.lib.described = Some(Described { id, note });
    }

    /// EDIT on a LIBRARY or FACTORY row of Part `part`'s browser.
    pub(crate) fn library_edit(&mut self, part: PartId, b: Browse) {
        let page = self.lib.page();
        let first = page.and_then(|p| p.entries().iter().flatten().next().map(|e| e.id));
        let last = page.and_then(|p| p.entries().iter().flatten().last().map(|e| e.id));
        let entry = match row(page, b.lib_cursor()) {
            LibRow::Prev => {
                return self.queue_opt(first.map(|id| Job::LibraryList(PageAt::Before(id))));
            }
            LibRow::Next => {
                return self.queue_opt(last.map(|id| Job::LibraryList(PageAt::After(id))));
            }
            LibRow::Empty => return,
            LibRow::Sound(e) => *e,
        };
        if let Some(e) = entry.err {
            return self.error(Line::new(e.message()));
        }
        let Some(file) = self.lib.page.file(&entry) else {
            return;
        };
        let src = LibraryPart { part, file };
        match ReplaceGuard::check(&self.project, self.template, src) {
            Ok(c) => self.queue(Job::LibraryLoad(c)),
            Err(n) => self.ask(Ask::LoadLibrary(LibraryAsk::new(n))),
        }
    }

    fn queue_opt(&mut self, j: Option<Job>) {
        if let Some(j) = j {
            self.queue(j);
        }
    }

    /// NAMING for SAVE TO LIBRARY of `part`'s Sound, pre-filled with its
    /// name; from the Sound rung, it closes back to `back`.
    pub(crate) fn name_for_library(&mut self, part: PartId, back: Option<Browse>) {
        let n = self.project.part(part).sound.name;
        let f = NamingFor::SaveToLibrary(part, back);
        self.name(SAVE_TO_LIB_LIST, f, n.as_str());
    }

    /// MENU while a load is pending: dropped, so nothing was published.
    pub(crate) fn cancel_library(&mut self) {
        if self.lib.load.take().is_some() {
            self.src.unhold();
            self.error(Line::new(CANCELLED));
            self.lib_ended();
        }
    }

    /// SAVE PART FIRST's confirmed load queued; the Stale Parts it left
    /// are asked about once it ends. Refused, they are asked now.
    pub(crate) fn queue_library(&mut self, c: Confirmed<LibraryPart>, stale: Option<Ask>) {
        self.queue(Job::LibraryLoad(c));
        match (self.job.is_some(), stale) {
            (true, s) => self.lib.after = s.and_then(|a| a.stale_parts()),
            (false, Some(a)) => self.ask(a),
            (false, None) => {}
        }
    }

    /// A load over, whatever its outcome: what SAVE PART FIRST left Stale.
    fn lib_ended(&mut self) {
        if self.lib.busy() {
            return;
        }
        if let Some((parts, slot)) = self.lib.after.take()
            && let Some(a) = Ask::update_stale(parts, slot, &self.project)
        {
            self.ask(a);
        }
    }

    /// Page `at` of the tab's shelf read into the one page static; on the
    /// browser, the cursor lands on it: the top for the first, else the
    /// Sound nearest where it came from.
    pub(crate) fn lib_list<S: Store>(&mut self, card: &mut Card, store: &mut S, at: PageAt) {
        let shelf = self.lib.tab.shelf().unwrap_or(Shelf::User);
        let CardOut { out, event } = list_library(card, store, shelf, at, &mut self.lib.page);
        // A new listing may be of other files: each description peeked anew.
        self.lib.described = None;
        self.saw(event);
        // The rows just read are the new card's: its event is no news.
        // A failure is listed too, as its row, so it isn't retried a pass.
        self.lib.listed = true;
        self.lib.err = out.err();
        if let Some((p, b)) = self.loc.browse() {
            let page = &self.lib.page;
            let (first, last) = sound_rows(page);
            let to = match at {
                PageAt::First => 0,
                PageAt::After(_) => first,
                PageAt::Before(_) => last,
            };
            // A new page: its scroll starts at the top, not the last page's.
            let n = rows(Some(page));
            self.loc = Location::sound_at(p, b.lib_at(0, n).lib_at(to, n));
            self.browser_dirty = true;
        }
    }

    /// `part`'s Sound to the library as `name`; then its page, read again.
    #[inline(never)]
    pub(crate) fn lib_save<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        part: PartId,
        name: SoundName,
    ) -> Option<Job> {
        let p = &self.project;
        let CardOut { out, event } =
            save_library_part(card, store, p, part, name, &about_page::THIS);
        self.saw(event);
        match out {
            Ok(id) => {
                // Saved as it sounds: CC 64 is this Sound now (ADR 0263).
                self.project.reset_shift(part);
                self.toast.show(Toast {
                    text: Line::new(SAVED),
                    ms: Toast::SAVED_MS,
                });
                self.lib.forget();
                // On the browser, the page holding it, the cursor on it:
                // page 1 for a top + 1 id. Elsewhere, LIBRARY lists anew.
                self.loc.browse()?;
                let at = LibId::new(id.get() + 1).map_or(PageAt::First, PageAt::After);
                Some(Job::LibraryList(at))
            }
            Err(n) => {
                self.lib_note(n);
                None
            }
        }
    }

    /// A library refusal: its words; a file or card that moved drops the
    /// page.
    fn lib_note(&mut self, n: LibNote) {
        if matches!(
            n,
            LibNote::Changed | LibNote::Gone | LibNote::Store(StoreError::VolumeChanged(_))
        ) {
            self.lib.forget();
        }
        self.error(Line::new(n.message()));
    }

    /// A confirmed load: the file into scratch, the budget, then its
    /// source streamed (pending) or straight to `adopt`.
    #[inline(never)]
    pub(crate) fn lib_load<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        go: Confirmed<LibraryPart>,
    ) {
        self.lib_read(card, store, go);
        self.lib_ended();
    }

    #[inline(never)]
    fn lib_read<S: Store>(&mut self, card: &mut Card, store: &mut S, go: Confirmed<LibraryPart>) {
        let file = go.target().file;
        let CardOut { out, event } = read_library_sound(card, store, &mut self.project, file);
        self.saw(event);
        let read = match out {
            Ok(r) => r,
            Err(n) => return self.lib_note(n),
        };
        if let Err(f) = self.project.library_needs(read.source) {
            return self.refuse(f);
        }
        let Some(need) = read.source.filter(SourceNeed::readable) else {
            let staged = read.source.map(|n| Staged::Missing(n.r));
            return self.lib_adopt(go, read, staged);
        };
        match self.stream_size(card, store, file.vol, need) {
            // Gone or changed since the read: it loads absent.
            None => self.lib_adopt(go, read, Some(Staged::Missing(need.r))),
            Some(size) => match SourceLoad::start(&mut self.project, need.r.stem(), size) {
                Some(stream) => {
                    debug_assert!(
                        !self.src.busy(),
                        "a library load starts beside no SOURCE job"
                    );
                    self.src.hold(stream);
                    self.lib.load = Some(PendingLoad { go, read });
                }
                None => self.error(Line::new(READ_FAILED)),
            },
        }
    }

    /// The size of `need`'s file on `vol`, if it is still the size the read
    /// costed it at.
    #[inline(never)]
    fn stream_size<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        vol: VolumeId,
        need: SourceNeed,
    ) -> Option<u32> {
        let name = source_file(need.r);
        let ran = card.run(store, |s, r| {
            let mut size = None;
            if r.volume() == vol {
                s.list(vol, Dir::Tapes, &mut |f, len| {
                    if f == name {
                        size = Some(len);
                    }
                })?;
            }
            Ok::<_, StoreError>(size)
        });
        let o = ran.ok()?;
        self.saw(Some(o.event));
        let size = o.result.ok()??;
        held_for_file_len(size).filter(|&h| h as usize == need.held)?;
        Some(size)
    }

    /// One pass of a pending load's source: on its last chunk, `adopt`.
    #[inline(never)]
    pub(crate) fn lib_step<S: Store>(&mut self, card: &mut Card, store: &mut S) {
        let Some(l) = self.lib.load.take() else {
            return;
        };
        let p = &mut self.project;
        let step = match self.src.staging() {
            Some(stream) => match card.run(store, |s, r| {
                Ok::<_, StoreError>(stream.step_staged(s, r, p))
            }) {
                Ok(o) => {
                    let step = o
                        .result
                        .unwrap_or(StagedStep::Failed(SourceNote::ReadFailed));
                    self.saw(Some(o.event));
                    step
                }
                Err(_) => StagedStep::Failed(SourceNote::ReadFailed),
            },
            None => StagedStep::Failed(SourceNote::ReadFailed),
        };
        if step == StagedStep::More {
            self.lib.load = Some(l);
            return;
        }
        self.src.unhold();
        match (step, l.read.source) {
            // Committed as the read costed it: the bytes staged are its file's.
            (StagedStep::Staged(got), Some(need)) if got.r == need.r => {
                self.lib_adopt(l.go, l.read, Some(Staged::Read(need)))
            }
            (StagedStep::Failed(w), _) => self.error(Line::new(w.words())),
            _ => self.error(Line::new(READ_FAILED)),
        }
        self.lib_ended();
    }

    /// The scratch Sound adopted into the Part, then its pages and one
    /// toast; a refusal changes nothing.
    fn lib_adopt(&mut self, go: Confirmed<LibraryPart>, read: LibRead, staged: Option<Staged>) {
        let part = go.target().part;
        let was = self.active_chain();
        match self.project.adopt(go, staged) {
            Ok(a) => {
                self.landed(part, was);
                if matches!(staged, Some(Staged::Missing(_))) {
                    self.error(Line::new(SOURCE_MISSING));
                } else if read.over {
                    self.error(Line::new(prompt::FORMANT_FULL));
                } else if read.skips > 0 {
                    self.notice(Line::new(SKIPPED));
                } else {
                    self.notice(loaded_to(a.slot));
                }
            }
            Err(AdoptError::Changed) => {
                self.error(Line::new(prompt::refused(&ReplaceError::Changed)))
            }
            Err(AdoptError::Full(f)) => self.refuse(f),
        }
    }

    /// Line 3 compares stamps with `this`, not the build's own, which the
    /// tree's state names.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn set_this_build_for_test(&mut self, this: BuildStamp) {
        self.lib.this = this;
    }

    /// The page on screen, if listed.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn library_page_for_test(&self) -> Option<&LibPage> {
        self.lib.page()
    }

    /// The row under LIBRARY's cursor, as its id.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn library_cursor_id_for_test(&self) -> Option<LibId> {
        let (_, b) = self.loc.browse()?;
        match row(self.lib.page(), b.lib_cursor()) {
            LibRow::Sound(e) => Some(e.id),
            _ => None,
        }
    }

    /// The page as if its volume were never read: its rows name no file.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn drop_library_volume_for_test(&mut self) {
        self.lib.page.vol = None;
    }

    /// The description FACTORY draws for the row under the cursor.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn factory_note_for_test(&self) -> Option<Note> {
        let (_, b) = self.loc.browse()?;
        match row(self.lib.page(), b.lib_cursor()) {
            LibRow::Sound(e) => self.lib.note_of(e),
            _ => None,
        }
    }

    /// A library load pending, as while its source streams: P1 from
    /// S0000001, confirmed now, no source.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn start_pending_library_load_for_test(&mut self) {
        use crate::project::{LibFile, LibSeen};
        let file = LibFile {
            id: LibId::new(1).expect("a user id"),
            vol: VolumeId {
                serial: 0,
                label: [b' '; 11],
            },
            seen: LibSeen {
                generation: None,
                name: None,
            },
        };
        let target = LibraryPart {
            part: PartId::ALL[0],
            file,
        };
        let go = match ReplaceGuard::check(&self.project, self.template, target) {
            Ok(c) => c,
            Err(n) => n.into_pending().anyway(&self.project),
        };
        self.lib.load = Some(PendingLoad {
            go,
            read: LibRead {
                skips: 0,
                over: false,
                source: None,
            },
        });
    }

    /// Why LIBRARY's page is empty, if its listing failed.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn library_err_for_test(&self) -> Option<&'static str> {
        self.lib.err().map(LibNote::message)
    }

    /// EDIT on the LIBRARY row under the cursor, whatever the keys.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn library_edit_for_test(&mut self) {
        if let Some((p, b)) = self.loc.browse() {
            self.library_edit(p, b);
        }
    }
}
