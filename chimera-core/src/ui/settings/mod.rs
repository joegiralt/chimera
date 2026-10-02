//! The SETTINGS menu: its tree is one static table (ADR 0066).

pub(crate) mod job;
pub mod leaves;
pub mod listing;
pub mod manage;
pub mod naming;
pub mod prompt;
pub mod tree;
pub mod view;

pub use job::CardCx;
pub use manage::MANAGE_COMMANDS;
pub use tree::{
    Act, Issue, Kind, PART_ROW, ROOT, Row, SAVE_AS_AT, Screen, issue, kind_at, path_of, row_at,
    rows, screen_path,
};

use chimera_hal::Controls;

use crate::name::{Name, ProjectName};
use crate::project::{
    FreshFile, PartFrom, PartId, PartSource, Pending, ProjectEntry, ProjectSource, Subject,
};
use crate::ui::hold::Presses;
use crate::ui::region::settings_key;
use job::{Then, ThenClear};
use listing::Chosen;
use naming::{NAME_MAX, Naming, NamingOut};
use prompt::{
    Answer, CardChanged, CardChangedAnswer, Choice, Clear, ClearAnswer, Delete, DeleteAnswer, Load,
    LoadAnswer, NameExists, NameExistsAnswer, PromptView, Replace, ReplaceAnswer, SaveOver,
    SaveOverAnswer, with_view,
};

/// SAVE PROJECT AS, named: the file it goes to and the load it is inside.
#[derive(Debug)]
pub(crate) struct SaveAs {
    pub fresh: FreshFile,
    pub name: ProjectName,
    pub then: Then,
}

/// An open prompt and what it holds until answered; dropping it is CANCEL.
#[derive(Debug)]
pub(crate) enum Ask {
    #[cfg_attr(
        not(any(test, feature = "test-support")),
        expect(dead_code, reason = "the PART branch, Task 13")
    )]
    ReplacePart(Pending<PartSource>, Choice<ReplaceAnswer>),
    /// LOAD over a Modified project; `to` names the file, `None` is NEW,
    /// and `clear` is CLEAR's save of NEW over the project's own file.
    LoadProject {
        pending: Pending<ProjectSource>,
        to: Option<ProjectName>,
        current: ProjectName,
        clear: ThenClear,
        choice: Choice<LoadAnswer>,
    },
    /// SAVE AS to a name `entry` already has.
    NameExists {
        save: SaveAs,
        entry: ProjectEntry,
        choice: Choice<NameExistsAnswer>,
    },
    /// A save refused: another card is in the slot. SAVE AS saves there,
    /// still inside the load `Then` holds.
    CardChanged(Then, Choice<CardChangedAnswer>),
    /// MANAGE's DELETE.
    Delete(Chosen, Choice<DeleteAnswer>),
    /// MANAGE's CLEAR, of the loaded project's own file or another's.
    Clear(Chosen, Choice<ClearAnswer>),
    /// MANAGE's SAVE TO.
    SaveOver(Chosen, Choice<SaveOverAnswer>),
}

/// A listed project as its prompt names it.
fn subject(c: &Chosen) -> Subject {
    let e = c.entry();
    e.name.map_or(Subject::File(e.id), Subject::Name)
}

/// A prompt's answer, with what it held.
#[derive(Debug)]
pub(crate) enum Answered {
    ReplacePart(Pending<PartSource>, Answer<ReplaceAnswer>),
    LoadProject {
        pending: Pending<ProjectSource>,
        to: Option<ProjectName>,
        clear: ThenClear,
        answer: Answer<LoadAnswer>,
    },
    NameExists(SaveAs, ProjectEntry, Answer<NameExistsAnswer>),
    CardChanged(Then, Answer<CardChangedAnswer>),
    Delete(Chosen, Answer<DeleteAnswer>),
    Clear(Chosen, Answer<ClearAnswer>),
    SaveOver(Chosen, Answer<SaveOverAnswer>),
}

/// A prompt after a frame's keys.
pub(crate) enum AskStep {
    Open(Ask),
    Answered(Answered),
}

impl Ask {
    #[cfg_attr(
        not(any(test, feature = "test-support")),
        expect(dead_code, reason = "the PART branch, Task 13")
    )]
    pub(crate) fn replace_part(p: Pending<PartSource>) -> Ask {
        Ask::ReplacePart(p, Choice::new())
    }

    pub(crate) fn with_view<R>(&self, f: impl FnOnce(&PromptView<'_>) -> R) -> R {
        match self {
            Ask::ReplacePart(p, c) => {
                let PartSource { part, from } = p.source();
                let to_init = matches!(from, PartFrom::Init(_));
                with_view(&Replace { part, to_init }, c, f)
            }
            Ask::LoadProject {
                to,
                current,
                choice,
                ..
            } => with_view(
                &Load {
                    to: *to,
                    current: *current,
                },
                choice,
                f,
            ),
            Ask::NameExists {
                save,
                entry,
                choice,
            } => with_view(
                &NameExists {
                    id: entry.id,
                    name: entry.name.unwrap_or(save.name),
                },
                choice,
                f,
            ),
            Ask::CardChanged(_, c) => with_view(&CardChanged, c, f),
            Ask::Delete(e, c) => with_view(&Delete { name: subject(e) }, c, f),
            Ask::Clear(e, c) => with_view(&Clear { name: subject(e) }, c, f),
            Ask::SaveOver(e, c) => with_view(&SaveOver { name: subject(e) }, c, f),
        }
    }

    /// What the panel shows, for its region.
    pub(crate) fn key(&self) -> u32 {
        self.with_view(|v| {
            let o = |i| v.options().get(i).map_or(&b""[..], |s: &&str| s.as_bytes());
            settings_key(&[
                v.question.as_bytes(),
                &[0],
                v.reason.as_bytes(),
                &[0],
                o(0),
                &[0],
                o(1),
                &[0],
                o(2),
                &[v.picked() as u8],
            ])
        })
    }

    fn input(self, c: &impl Controls, p: &Presses) -> AskStep {
        use AskStep::{Answered as Got, Open};
        match self {
            Ask::ReplacePart(pending, mut choice) => match choice.input(c, p) {
                Some(a) => Got(Answered::ReplacePart(pending, a)),
                None => Open(Ask::ReplacePart(pending, choice)),
            },
            Ask::LoadProject {
                pending,
                to,
                current,
                clear,
                mut choice,
            } => match choice.input(c, p) {
                Some(answer) => Got(Answered::LoadProject {
                    pending,
                    to,
                    clear,
                    answer,
                }),
                None => Open(Ask::LoadProject {
                    pending,
                    to,
                    current,
                    clear,
                    choice,
                }),
            },
            Ask::NameExists {
                save,
                entry,
                mut choice,
            } => match choice.input(c, p) {
                Some(a) => Got(Answered::NameExists(save, entry, a)),
                None => Open(Ask::NameExists {
                    save,
                    entry,
                    choice,
                }),
            },
            Ask::CardChanged(then, mut choice) => match choice.input(c, p) {
                Some(a) => Got(Answered::CardChanged(then, a)),
                None => Open(Ask::CardChanged(then, choice)),
            },
            Ask::Delete(e, mut choice) => match choice.input(c, p) {
                Some(a) => Got(Answered::Delete(e, a)),
                None => Open(Ask::Delete(e, choice)),
            },
            Ask::Clear(e, mut choice) => match choice.input(c, p) {
                Some(a) => Got(Answered::Clear(e, a)),
                None => Open(Ask::Clear(e, choice)),
            },
            Ask::SaveOver(e, mut choice) => match choice.input(c, p) {
                Some(a) => Got(Answered::SaveOver(e, a)),
                None => Open(Ask::SaveOver(e, choice)),
            },
        }
    }
}

/// Whose name NAMING edits.
#[derive(Debug)]
pub(crate) enum NamingFor {
    RenameLoaded,
    #[cfg_attr(
        not(any(test, feature = "test-support")),
        expect(dead_code, reason = "RENAME, Task 13")
    )]
    RenamePart(PartId),
    /// SAVE PROJECT AS to a file no pair had, inside a load or not.
    SaveAs(FreshFile, Then),
}

impl NamingFor {
    /// Over the boxes.
    pub(crate) fn title(&self) -> &'static str {
        match self {
            NamingFor::RenameLoaded | NamingFor::SaveAs(..) => "PROJECT NAME",
            NamingFor::RenamePart(_) => "SOUND NAME",
        }
    }

    /// The breadcrumb's last part while NAMING is open.
    pub(crate) fn crumb(&self) -> &'static str {
        match self {
            NamingFor::RenameLoaded | NamingFor::RenamePart(_) => "RENAME",
            NamingFor::SaveAs(..) => "SAVE AS",
        }
    }
}

/// What takes the keys over the screen: one at a time.
#[derive(Debug)]
pub(crate) enum Modal {
    Prompt(Ask),
    Naming(NamingFor, Naming),
}

pub(crate) enum ModalStep {
    Open(Modal),
    /// SEQ on an empty name: refused, NAMING stays.
    Refused(NamingFor, Naming),
    Done(Done),
}

pub(crate) enum Done {
    Answered(Answered),
    Named(NamingFor, Name<NAME_MAX>),
    Cancelled,
}

impl Modal {
    pub(crate) fn input(self, c: &impl Controls, p: &Presses) -> ModalStep {
        match self {
            Modal::Prompt(a) => match a.input(c, p) {
                AskStep::Answered(x) => ModalStep::Done(Done::Answered(x)),
                AskStep::Open(a) => ModalStep::Open(Modal::Prompt(a)),
            },
            Modal::Naming(f, mut n) => match n.input(c, p) {
                None => ModalStep::Open(Modal::Naming(f, n)),
                Some(NamingOut::Empty) => ModalStep::Refused(f, n),
                Some(NamingOut::Cancel) => ModalStep::Done(Done::Cancelled),
                Some(NamingOut::Save(name)) => ModalStep::Done(Done::Named(f, name)),
            },
        }
    }
}
