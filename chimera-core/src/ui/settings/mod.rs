//! The SETTINGS menu: its tree is one static table (ADR 0066).

pub(crate) mod job;
pub mod leaves;
pub mod listing;
pub mod naming;
pub mod prompt;
pub mod tree;
pub mod view;

pub use job::CardCx;
pub use tree::{
    Act, Issue, Kind, MANAGE_COMMANDS, PART_ROW, ROOT, Row, SAVE_AS_AT, Screen, issue, path_of,
    row_at, rows,
};

use chimera_hal::Controls;

use crate::name::{Name, ProjectName};
use crate::project::{
    FreshFile, PartFrom, PartId, PartSource, Pending, ProjectEntry, ProjectSource,
};
use crate::ui::hold::Presses;
use crate::ui::region::settings_key;
use job::Then;
use naming::{NAME_MAX, Naming, NamingOut};
use prompt::{
    Answer, CardChanged, CardChangedAnswer, Choice, Load, LoadAnswer, NameExists, NameExistsAnswer,
    PromptView, Replace, ReplaceAnswer, with_view,
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
    /// LOAD over a Modified project; `to` names the file, `None` is NEW.
    LoadProject {
        pending: Pending<ProjectSource>,
        to: Option<ProjectName>,
        current: ProjectName,
        choice: Choice<LoadAnswer>,
    },
    /// SAVE AS to a name `entry` already has.
    NameExists {
        save: SaveAs,
        entry: ProjectEntry,
        choice: Choice<NameExistsAnswer>,
    },
    /// A quick save refused: another card is in the slot.
    CardChanged(Choice<CardChangedAnswer>),
}

/// A prompt's answer, with what it held.
#[derive(Debug)]
pub(crate) enum Answered {
    ReplacePart(Pending<PartSource>, Answer<ReplaceAnswer>),
    LoadProject(Pending<ProjectSource>, Answer<LoadAnswer>),
    NameExists(SaveAs, ProjectEntry, Answer<NameExistsAnswer>),
    CardChanged(Answer<CardChangedAnswer>),
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
            Ask::CardChanged(c) => with_view(&CardChanged, c, f),
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
                mut choice,
            } => match choice.input(c, p) {
                Some(a) => Got(Answered::LoadProject(pending, a)),
                None => Open(Ask::LoadProject {
                    pending,
                    to,
                    current,
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
            Ask::CardChanged(mut choice) => match choice.input(c, p) {
                Some(a) => Got(Answered::CardChanged(a)),
                None => Open(Ask::CardChanged(choice)),
            },
        }
    }
}

/// Whose name NAMING edits.
#[derive(Debug)]
pub(crate) enum NamingFor {
    #[cfg_attr(
        not(any(test, feature = "test-support")),
        expect(dead_code, reason = "RENAME, Tasks 12 and 13")
    )]
    RenameLoaded,
    #[cfg_attr(
        not(any(test, feature = "test-support")),
        expect(dead_code, reason = "RENAME, Tasks 12 and 13")
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
