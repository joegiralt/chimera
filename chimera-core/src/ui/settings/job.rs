//! Card work the keys ask for: queued by `handle_input`, run by
//! `card_work` after it, under BUSY. A frame never touches the card.

use chimera_hal::store::{Store, StoreError};

use crate::name::ProjectName;
use crate::project::{
    self, CardOut, Confirmed, DeleteTarget, FreshFile, LoadLink, OverwriteTarget, Pending, Project,
    ProjectNote, ProjectSource, SaveTo, Swap,
};
use crate::storage::{Card, CardEvent, SystemSettings, SystemSync};
use crate::ui::UiState;
use crate::ui::nav::{ListAt, Location};

use super::listing::Validity;
use super::naming::proposed_name;
use super::prompt::Choice;
use super::{Ask, NamingFor, SAVE_AS_AT, SaveAs, Screen, screen_path};

/// SETTINGS › PROJECT, the bar on SAVE PROJECT AS.
pub(crate) const SAVE_AS_LIST: ListAt = ListAt::at(&[SAVE_AS_AT.0], SAVE_AS_AT.1);

/// A load waiting on a save (SAVE THEN LOAD), and the name its prompt
/// showed, should it have to ask again.
#[derive(Debug)]
pub(crate) struct LoadAfter {
    pub pending: Pending<ProjectSource>,
    pub to: Option<ProjectName>,
}

/// The load a save runs inside, if any.
pub(crate) type Then = Option<LoadAfter>;

/// One piece of card work.
#[derive(Debug)]
pub(crate) enum Job {
    /// Lists the card, then enters the Screen.
    List(Screen),
    /// SAVE over the project's own file.
    QuickSave(Then),
    /// SAVE PROJECT AS: the card listed, its next id, then NAMING.
    Fresh(Then),
    /// NAMING's answer: NAME EXISTS asks of the whole card, else it saves.
    Named(SaveAs),
    /// A save under the name given; the project takes it once saved.
    Save(SaveTo, ProjectName, Then),
    Load(Confirmed<ProjectSource>),
    #[expect(dead_code, reason = "MANAGE, Task 12")]
    Delete(Confirmed<DeleteTarget>),
    #[expect(dead_code, reason = "MANAGE, Task 12")]
    Clear(Confirmed<OverwriteTarget>),
}

/// What card work borrows from the shell.
pub struct CardCx<'a, S: Store> {
    pub card: &'a mut Card,
    pub store: &'a mut S,
    pub sync: &'a mut SystemSync,
    pub settings: &'a mut SystemSettings,
}

impl UiState {
    /// Card work is queued: the shell shows BUSY before `card_work`.
    pub fn card_pending(&self) -> bool {
        self.job.is_some() || self.wants_list()
    }

    /// LOAD or MANAGE on screen over a listing something moved.
    fn wants_list(&self) -> bool {
        self.listing.validity() == Validity::Stale
            && matches!(
                self.screen(),
                Some(Screen::LoadProject | Screen::ManageProjects)
            )
    }

    /// Once a frame, after `handle_input`: the queued job, then a re-list
    /// if LOAD or MANAGE shows a stale one. A load's swap goes to
    /// `publish` (settle, then publish), before any SYSTEM write.
    pub fn card_work<S: Store, R>(
        &mut self,
        mut cx: CardCx<'_, S>,
        link: &LoadLink,
        publish: impl FnOnce(Swap, &Project) -> R,
    ) -> Option<R> {
        let mut publish = Some(publish);
        let mut out = None;
        let mut next = self.job.take();
        while let Some(job) = next {
            next = self.run(job, &mut cx, link, &mut publish, &mut out);
        }
        if self.wants_list() {
            let _ = self.relist(cx.card, cx.store);
        }
        out
    }

    /// One job; what runs next in the same call.
    fn run<S: Store, R, F: FnOnce(Swap, &Project) -> R>(
        &mut self,
        job: Job,
        cx: &mut CardCx<'_, S>,
        link: &LoadLink,
        publish: &mut Option<F>,
        out: &mut Option<R>,
    ) -> Option<Job> {
        if !matches!(job, Job::List(_)) {
            self.listing.mark_stale();
        }
        match job {
            Job::List(s) => {
                let _ = self.relist(cx.card, cx.store);
                self.go(Location::settings_at(screen_path(s), 0));
                None
            }
            Job::QuickSave(then) => {
                let name = self.project.meta().name();
                match self.saved_to(cx, SaveTo::Own, name) {
                    ProjectNote::NoFile => {
                        if then.is_none() {
                            self.go(SAVE_AS_LIST.location());
                        }
                        Some(Job::Fresh(then))
                    }
                    n => self.after_save(n, then),
                }
            }
            Job::Fresh(then) => {
                match self.relist(cx.card, cx.store) {
                    Ok(fresh) => {
                        let at = self.loc.settings().and_then(|s| s.list());
                        let start = proposed_name(fresh.file().id());
                        let f = NamingFor::SaveAs(fresh, then);
                        self.name(at.unwrap_or(SAVE_AS_LIST), f, start.as_str());
                    }
                    Err(n) => self.show_note(n),
                }
                None
            }
            Job::Named(save) => self.check_name(cx, save),
            Job::Save(to, name, then) => {
                let n = self.saved_to(cx, to, name);
                self.after_save(n, then)
            }
            Job::Load(go) => {
                if let Some(f) = publish.take() {
                    let (r, event) = self.load_with(cx, go, link, f);
                    self.saw(event);
                    *out = r.or(out.take());
                }
                None
            }
            Job::Delete(c) => {
                let e = self.delete_project(cx.card, cx.store, cx.sync, cx.settings, c);
                self.saw(e);
                None
            }
            Job::Clear(c) => {
                let CardOut { out: r, event } =
                    project::clear_project(cx.card, cx.store, &self.project, c);
                self.saw(event);
                if let Err(n) = r {
                    self.show_note(n);
                }
                None
            }
        }
    }

    /// NAME EXISTS against the card in the slot: the listing, then any ids
    /// past it. On another card the save goes ahead, to be refused there.
    fn check_name<S: Store>(&mut self, cx: &mut CardCx<'_, S>, save: SaveAs) -> Option<Job> {
        // A card that can't be read drops the save, and any load it is in.
        match self.relist(cx.card, cx.store) {
            Err(n) if n != ProjectNote::NoIds => {
                self.show_note(n);
                return None;
            }
            _ => {}
        }
        let here = self.listing.vol() == Some(save.fresh.file().vol());
        let mut taken = self.listing.named(&save.name).filter(|_| here);
        if here && taken.is_none() && self.listing.more() {
            let after = self
                .listing
                .entry(self.listing.len() - 1)
                .map_or(0, |e| e.id.get());
            let CardOut { out, event } = project::find_named(cx.card, cx.store, &save.name, after);
            self.saw(event);
            match out {
                Ok(e) => taken = e,
                Err(n) => {
                    self.show_note(n);
                    return None;
                }
            }
        }
        match taken {
            Some(entry) => {
                self.ask(Ask::NameExists {
                    save,
                    entry,
                    choice: Choice::new(),
                });
                None
            }
            None => Some(Job::Save(SaveTo::Fresh(save.fresh), save.name, save.then)),
        }
    }

    /// A save's note, its event seen; a landed save becomes SYSTEM's last.
    fn saved_to<S: Store>(
        &mut self,
        cx: &mut CardCx<'_, S>,
        to: SaveTo,
        name: ProjectName,
    ) -> ProjectNote {
        let CardOut { out, event } = self.save(cx.card, cx.store, cx.sync, cx.settings, to, name);
        self.saw(event);
        out
    }

    /// After a save: the load it was inside runs only if it landed; another
    /// card in the slot offers SAVE AS there, the load still pending.
    fn after_save(&mut self, n: ProjectNote, then: Then) -> Option<Job> {
        match n {
            ProjectNote::Saved(_) => {
                self.show_note(n);
                let LoadAfter { pending, to } = then?;
                match pending.save_then(&self.project, self.template) {
                    Ok(c) => Some(Job::Load(c)),
                    Err(again) => {
                        let current = self.project.meta().name();
                        self.ask(Ask::LoadProject {
                            pending: again.into_pending(),
                            to,
                            current,
                            choice: Choice::new(),
                        });
                        None
                    }
                }
            }
            ProjectNote::Card {
                err: StoreError::VolumeChanged(_),
                ..
            } => {
                self.ask(Ask::CardChanged(then, Choice::new()));
                None
            }
            n => {
                self.show_note(n);
                None
            }
        }
    }

    /// Another card in the slot: what was listed belongs to the old one.
    pub(crate) fn saw(&mut self, e: Option<CardEvent>) {
        if let Some(CardEvent::Swapped { .. }) = e {
            self.listing.swapped();
        }
    }

    /// Re-reads the card into the listing in one pass, keeps the bar on its
    /// rows, and gives the file a save as would take.
    fn relist<S: Store>(
        &mut self,
        card: &mut Card,
        store: &mut S,
    ) -> Result<FreshFile, ProjectNote> {
        let listing = &mut self.listing;
        let mut begun = false;
        let out = project::list_projects(card, store, &mut |e| {
            if !begun {
                listing.begin(e.vol);
                begun = true;
            }
            listing.push(e);
        });
        let vol = match *card {
            Card::Ready(v) => Some(v),
            Card::Failed { last, .. } => last,
            Card::Absent => None,
        };
        match (out.event, vol) {
            (Some(_), _) if begun => listing.ended(out.more),
            (Some(_), Some(v)) => {
                listing.begin(v);
                listing.ended(out.more);
            }
            _ => {
                let err = match out.note {
                    Some(ProjectNote::Card { err, .. }) => err,
                    _ => StoreError::Io,
                };
                listing.unreadable(err);
            }
        }
        // The rows just read are the new card's: its event is no news.
        if let (Some(_), Some(n)) = (out.event, out.note) {
            self.show_note(n);
        }
        if let Some(s) = self.loc.settings()
            && s.screen().is_some()
        {
            let rows = self.cx().dyn_rows;
            self.go(self.loc.with_row_within(rows));
        }
        out.fresh.ok_or(out.note.unwrap_or(ProjectNote::NoIds))
    }
}
