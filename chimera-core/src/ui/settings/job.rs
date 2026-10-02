//! Card work the keys ask for: queued by `handle_input`, run by
//! `card_work` after it, under BUSY. A frame never touches the card.

use chimera_hal::store::{Store, StoreError};

use crate::project::{
    self, CardOut, Confirmed, DeleteTarget, LoadLink, OverwriteTarget, Pending, Project,
    ProjectNote, ProjectSource, SaveTo, Swap,
};
use crate::storage::{Card, CardEvent, SystemSettings, SystemSync};
use crate::ui::UiState;
use crate::ui::nav::Location;

use super::listing::Validity;
use super::naming::proposed_name;
use super::{Ask, NamingFor, SAVE_AS_AT, Screen, path_of};

/// The load a save runs inside (SAVE THEN LOAD), if any.
pub(crate) type Then = Option<Pending<ProjectSource>>;

/// One piece of card work.
#[derive(Debug)]
pub(crate) enum Job {
    /// Lists the card, then enters the Screen.
    List(Screen),
    /// SAVE over the project's own file.
    QuickSave(Then),
    /// SAVE PROJECT AS: a new id, then NAMING.
    Fresh(Then),
    Save(SaveTo, Then),
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
            self.relist(cx.card, cx.store);
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
                self.relist(cx.card, cx.store);
                let (path, depth) = path_of(s);
                self.go(Location::settings_at(&path[..depth], 0));
                None
            }
            Job::QuickSave(then) => {
                let n = self.saved_to(cx, SaveTo::Own);
                match (n, then) {
                    (ProjectNote::NoFile, then) => {
                        if then.is_none() {
                            self.go(save_as_place());
                        }
                        Some(Job::Fresh(then))
                    }
                    (
                        ProjectNote::Card {
                            err: StoreError::VolumeChanged(_),
                            ..
                        },
                        None,
                    ) => {
                        self.ask(Ask::CardChanged(Default::default()));
                        None
                    }
                    (n, then) => self.after_save(n, then),
                }
            }
            Job::Fresh(then) => {
                let CardOut { out: fresh, event } = project::new_project_id(cx.card, cx.store);
                self.saw(event);
                match fresh {
                    Ok(fresh) => {
                        // NAME EXISTS asks of the card as it is now.
                        self.relist(cx.card, cx.store);
                        let at = self
                            .loc
                            .settings()
                            .and_then(|s| s.list())
                            .or_else(|| save_as_place().settings().and_then(|s| s.list()));
                        let start = proposed_name(fresh.file().id());
                        if let Some(at) = at {
                            self.name(at, NamingFor::SaveAs(fresh, then), start.as_str());
                        }
                    }
                    Err(n) => self.show_note(n),
                }
                None
            }
            Job::Save(to, then) => {
                let n = self.saved_to(cx, to);
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

    /// A save's note, its event seen; a landed save becomes SYSTEM's last.
    fn saved_to<S: Store>(&mut self, cx: &mut CardCx<'_, S>, to: SaveTo) -> ProjectNote {
        let CardOut { out, event } = self.save(cx.card, cx.store, cx.sync, cx.settings, to);
        self.saw(event);
        out
    }

    /// The save's toast; the load it was inside runs only if it landed.
    fn after_save(&mut self, n: ProjectNote, then: Then) -> Option<Job> {
        self.show_note(n);
        match (n, then) {
            (ProjectNote::Saved(_), Some(p)) => p
                .save_then(&self.project, self.template)
                .ok()
                .map(Job::Load),
            _ => None,
        }
    }

    /// Another card in the slot: what was listed belongs to the old one.
    pub(crate) fn saw(&mut self, e: Option<CardEvent>) {
        if let Some(CardEvent::Swapped { .. }) = e {
            self.listing.swapped();
        }
    }

    /// Re-reads the card into the listing, and keeps the bar on its rows.
    fn relist<S: Store>(&mut self, card: &mut Card, store: &mut S) {
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
            (Some(_), Some(v)) if !begun => listing.begin(v),
            (Some(_), _) if begun => {}
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
    }
}

/// SETTINGS › PROJECT, the bar on SAVE PROJECT AS.
fn save_as_place() -> Location {
    Location::settings_at(&[SAVE_AS_AT.0], SAVE_AS_AT.1)
}
