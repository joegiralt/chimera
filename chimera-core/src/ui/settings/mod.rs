//! The SETTINGS menu: its tree is one static table (ADR 0066).

pub mod leaves;
pub mod naming;
pub mod prompt;
pub mod tree;
pub mod view;

pub use tree::{
    Act, Issue, Kind, MANAGE_COMMANDS, PART_ROW, ROOT, Row, Screen, issue, row_at, rows,
};

use chimera_hal::Controls;

use crate::name::Name;
use crate::project::{PartFrom, PartId, PartSource, Pending};
use crate::ui::hold::Presses;
use crate::ui::region::settings_key;
use naming::{NAME_MAX, Naming, NamingOut};
use prompt::{Answer, Choice, PromptView, Replace, ReplaceAnswer, with_view};

/// An open prompt and what it holds until answered; dropping it is CANCEL.
/// One variant per prompt as Tasks 11–13 wire them.
#[derive(Debug)]
pub enum Ask {
    ReplacePart(Pending<PartSource>, Choice<ReplaceAnswer>),
}

/// A prompt's answer, with what it held.
#[derive(Debug)]
pub enum Answered {
    ReplacePart(Pending<PartSource>, Answer<ReplaceAnswer>),
}

impl Ask {
    pub fn replace_part(p: Pending<PartSource>) -> Ask {
        Ask::ReplacePart(p, Choice::new())
    }

    pub fn with_view<R>(&self, f: impl FnOnce(&PromptView<'_>) -> R) -> R {
        match self {
            Ask::ReplacePart(p, c) => {
                let PartSource { part, from } = p.source();
                let to_init = matches!(from, PartFrom::Init(_));
                with_view(&Replace { part, to_init }, c, f)
            }
        }
    }

    /// What the panel shows, for its region.
    pub fn key(&self) -> u32 {
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

    /// The answer, or the prompt still open.
    fn input(self, c: &impl Controls, p: &Presses) -> Result<Answered, Ask> {
        match self {
            Ask::ReplacePart(pending, mut choice) => match choice.input(c, p) {
                Some(a) => Ok(Answered::ReplacePart(pending, a)),
                None => Err(Ask::ReplacePart(pending, choice)),
            },
        }
    }
}

/// Whose name NAMING edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamingFor {
    RenameLoaded,
    RenamePart(PartId),
}

impl NamingFor {
    /// Over the boxes.
    pub fn title(self) -> &'static str {
        match self {
            NamingFor::RenameLoaded => "PROJECT NAME",
            NamingFor::RenamePart(_) => "SOUND NAME",
        }
    }

    /// The breadcrumb's last part while NAMING is open.
    pub fn crumb(self) -> &'static str {
        match self {
            NamingFor::RenameLoaded | NamingFor::RenamePart(_) => "RENAME",
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
    Empty(Modal),
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
                Ok(x) => ModalStep::Done(Done::Answered(x)),
                Err(a) => ModalStep::Open(Modal::Prompt(a)),
            },
            Modal::Naming(f, mut n) => match n.input(c, p) {
                None => ModalStep::Open(Modal::Naming(f, n)),
                Some(NamingOut::Empty) => ModalStep::Empty(Modal::Naming(f, n)),
                Some(NamingOut::Cancel) => ModalStep::Done(Done::Cancelled),
                Some(NamingOut::Save(name)) => ModalStep::Done(Done::Named(f, name)),
            },
        }
    }
}
