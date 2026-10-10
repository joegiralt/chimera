//! The Sound rung's LIBRARY tab (library spec § Browser, § Load): the page
//! as listed, the tab B turns, and a load in flight. A pending load holds
//! the pool's scratch and maybe its staging, so while it waits no other
//! card work starts.

use core::fmt::Write;

use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::pixelcolor::Rgb565;

use chimera_hal::store::{Dir, Store, StoreError, VolumeId};

use crate::name::SoundName;
use crate::project::{
    AdoptError, CardOut, Confirmed, LibEntry, LibNote, LibPage, LibRead, LibraryPart, Line, PageAt,
    PartId, PartSet, ReplaceError, ReplaceGuard, SlotId, SourceNeed, Staged, list_library,
    read_library_sound, save_library_sound,
};
use crate::storage::{Card, LibId, held_for_file_len, source_file};
use crate::ui::UiState;
use crate::ui::about_page;
use crate::ui::browser::{Browse, VISIBLE_ROWS, row_y};
use crate::ui::busy::Toast;
use crate::ui::components;
use crate::ui::draw;
use crate::ui::fmt::FmtBuf;
use crate::ui::nav::{ListAt, Location};
use crate::ui::settings::job::Job;
use crate::ui::settings::prompt;
use crate::ui::settings::replace::LibraryAsk;
use crate::ui::settings::{Ask, NamingFor, PART_ROW, SAVE_TO_LIB_ROW};
use crate::ui::source_cell::{READ_FAILED, SOURCE_MISSING, SourceLoad, StagedStep};
use crate::ui::theme;

/// What the Sound rung lists: kept for the session, PROJECT at boot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Project,
    Library,
}

/// A library load between its read and `adopt`: the confirmation and
/// what the read said. Its source streams in the SOURCE cell's job slot
/// (`SourceCell::hold`), which it shares, as the two never stream at once.
pub(crate) struct PendingLoad {
    pub go: Confirmed<LibraryPart>,
    pub read: LibRead,
}

pub(crate) struct LibState {
    page: LibPage,
    listed: bool,
    /// Why the last listing failed: its row says so until listed again,
    /// on the next tab or rung entry.
    err: Option<LibNote>,
    pub load: Option<PendingLoad>,
    pub tab: Tab,
    /// SAVE PART FIRST's Stale Parts and slot: asked about once the load
    /// it ran before ends, not under it.
    after: Option<(PartSet, SlotId)>,
}

impl LibState {
    pub const NEW: LibState = LibState {
        page: LibPage::EMPTY,
        listed: false,
        err: None,
        load: None,
        tab: Tab::Project,
        after: None,
    };

    /// A load is pending: no other card work starts.
    pub fn busy(&self) -> bool {
        self.load.is_some()
    }

    /// The page goes: listed again when LIBRARY is next on screen.
    pub fn forget(&mut self) {
        self.listed = false;
    }

    pub fn page(&self) -> Option<&LibPage> {
        self.listed.then_some(&self.page)
    }

    /// Why the page on screen is empty: the card couldn't be listed.
    pub fn err(&self) -> Option<LibNote> {
        self.err.filter(|_| self.listed)
    }

    /// A failed listing is tried again: LIBRARY entered anew.
    pub fn entered(&mut self) {
        if self.err.is_some() {
            self.forget();
        }
    }
}

/// A LIBRARY row.
#[derive(Clone, Copy, Debug)]
pub enum LibRow<'a> {
    /// `‹ NEWER`: the page above.
    Newer,
    Sound(&'a LibEntry),
    /// `OLDER ›`: the page below.
    Older,
    /// `NO SOUNDS YET`, or an unreadable card: EDIT does nothing.
    Empty,
}

/// How many rows the page shows: at least one.
pub fn rows(page: Option<&LibPage>) -> usize {
    let Some(p) = page else { return 1 };
    let n = usize::from(p.newer) + p.entries().len() + usize::from(p.older);
    n.max(1)
}

/// Row `i` of the page.
pub fn row(page: Option<&LibPage>, i: usize) -> LibRow<'_> {
    let Some(p) = page else {
        return LibRow::Empty;
    };
    let first = usize::from(p.newer);
    let n = p.entries().len();
    match i {
        0 if p.newer => LibRow::Newer,
        i if i >= first && i < first + n => p.entries()[i - first]
            .as_ref()
            .map_or(LibRow::Empty, LibRow::Sound),
        i if p.older && i == first + n => LibRow::Older,
        _ => LibRow::Empty,
    }
}

/// The first and last Sound rows: where a page lands the cursor.
pub fn sound_rows(page: &LibPage) -> (usize, usize) {
    let first = usize::from(page.newer);
    (first, first + page.entries().len().saturating_sub(1))
}

/// Placeholder rows; the browser's LIBRARY look is drawn later. With no
/// row, `err`'s words, or `NO SOUNDS YET`.
pub(crate) fn draw<D>(d: &mut D, lib: &LibState, part: PartId, cursor: usize, scroll: usize)
where
    D: DrawTarget<Color = Rgb565>,
{
    let mut p = FmtBuf::new();
    let _ = write!(p, "P{}", part.index() + 1);
    components::title_to(d, "LIBRARY", p.as_str());
    let page = lib.page();
    let total = rows(page);
    for i in 0..VISIBLE_ROWS {
        let at = scroll + i;
        if at >= total {
            break;
        }
        let y = row_y(i);
        let on = at == cursor;
        if on {
            draw::pill(d, 8, y - 16, 224, 22, theme::ACCENT);
        }
        let ink = if on { theme::BG } else { theme::INK };
        let name = match row(page, at) {
            LibRow::Newer => components::upper("‹ NEWER"),
            LibRow::Older => components::upper("OLDER ›"),
            LibRow::Empty => components::upper(lib.err().map_or("NO SOUNDS YET", LibNote::message)),
            LibRow::Sound(e) => match e.seen.name {
                Some(n) => components::upper(n.as_str()),
                None => components::upper("—"),
            },
        };
        draw::text(d, &theme::FONT_VALUE, name.as_str(), 18, y, ink);
    }
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
    pub fn lib_tab(&self) -> Tab {
        self.lib.tab
    }

    /// The library's page goes (a console `put` or `rm`): listed again
    /// when LIBRARY is next on screen.
    pub fn forget_library(&mut self) {
        self.lib.forget();
    }

    /// LIBRARY on screen with no page: listed on the next pass.
    pub(crate) fn wants_lib_list(&self) -> bool {
        self.lib.tab == Tab::Library
            && !self.lib.listed
            && !self.lib.busy()
            && self.loc.browse().is_some()
    }

    /// EDIT on a LIBRARY row of Part `part`'s browser.
    pub(crate) fn library_edit(&mut self, part: PartId, b: Browse) {
        let page = self.lib.page();
        let first = page.and_then(|p| p.entries().iter().flatten().next().map(|e| e.id));
        let last = page.and_then(|p| p.entries().iter().flatten().last().map(|e| e.id));
        let entry = match row(page, b.lib_cursor()) {
            LibRow::Newer => {
                return self.queue_opt(first.map(|id| Job::LibraryList(PageAt::Above(id))));
            }
            LibRow::Older => {
                return self.queue_opt(last.map(|id| Job::LibraryList(PageAt::Below(id))));
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

    /// Page `at` read into the cache; on the browser, the cursor lands on
    /// it: the top for the newest, else the Sound nearest where it came
    /// from.
    pub(crate) fn lib_list<S: Store>(&mut self, card: &mut Card, store: &mut S, at: PageAt) {
        let CardOut { out, event } = list_library(card, store, at, &mut self.lib.page);
        self.saw(event);
        // The rows just read are the new card's: its event is no news.
        // A failure is listed too, as its row, so it isn't retried a pass.
        self.lib.listed = true;
        self.lib.err = out.err();
        if let Some((p, b)) = self.loc.browse() {
            let page = &self.lib.page;
            let (first, last) = sound_rows(page);
            let to = match at {
                PageAt::Newest => 0,
                PageAt::Below(_) => first,
                PageAt::Above(_) => last,
            };
            self.loc = Location::sound_at(p, b.lib_at(to, rows(Some(page))));
            self.browser_dirty = true;
        }
    }

    /// `part`'s Sound to the library as `name`; then its page, read again.
    pub(crate) fn lib_save<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        part: PartId,
        name: SoundName,
    ) -> Option<Job> {
        let s = &self.project.part(part).sound;
        let CardOut { out, event } = save_library_sound(card, store, s, name, &about_page::THIS);
        self.saw(event);
        match out {
            Ok(id) => {
                self.toast.show(Toast {
                    text: Line::new(SAVED),
                    ms: Toast::SAVED_MS,
                });
                self.lib.forget();
                // On the browser, the page holding it, the cursor on it:
                // page 1 for a top + 1 id. Elsewhere, LIBRARY lists anew.
                self.loc.browse()?;
                let at = LibId::new(id.get() + 1).map_or(PageAt::Newest, PageAt::Below);
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
    pub(crate) fn lib_load<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
        go: Confirmed<LibraryPart>,
    ) {
        self.lib_read(card, store, go);
        self.lib_ended();
    }

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
            Some(size) => match SourceLoad::start(&mut self.project, need.r.crc, size) {
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
                    let step = o.result.unwrap_or(StagedStep::Failed(READ_FAILED));
                    self.saw(Some(o.event));
                    step
                }
                Err(_) => StagedStep::Failed(READ_FAILED),
            },
            None => StagedStep::Failed(READ_FAILED),
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
            (StagedStep::Failed(w), _) => self.error(Line::new(w)),
            _ => self.error(Line::new(READ_FAILED)),
        }
        self.lib_ended();
    }

    /// The scratch Sound adopted into the Part, then its pages and one
    /// toast; a refusal changes nothing.
    fn lib_adopt(&mut self, go: Confirmed<LibraryPart>, read: LibRead, staged: Option<Staged>) {
        let part = go.target().part;
        let was = self.active_engine();
        match self.project.adopt(go, staged) {
            Ok(a) => {
                self.landed(part, was);
                if matches!(staged, Some(Staged::Missing(_))) {
                    self.error(Line::new(SOURCE_MISSING));
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
