//! A replace waiting on its prompt (#258): the prompt's `Pending` and its
//! `Choice` together, so only a SEQ tap on the confirming pill makes the
//! `Confirmed`. DELETE, CLEAR, SAVE OVER, OVERWRITE THAT ONE and ENTER DFU
//! confirm through `said` the same way. Nothing else holds a `Said`:
//!
//! ```compile_fail,E0423
//! use chimera_core::ui::settings::replace::said::Said;
//! let _: Said<chimera_core::project::DeleteTarget> = Said(core::marker::PhantomData);
//! ```
//!
//! ```compile_fail,E0451
//! use chimera_core::project::{PartSource, Pending};
//! use chimera_core::ui::settings::prompt::Choice;
//! use chimera_core::ui::settings::replace::PartAsk;
//! fn forge(pending: Pending<PartSource>) -> PartAsk {
//!     PartAsk { pending, choice: Choice::new() }
//! }
//! ```

pub mod said;

use chimera_hal::Controls;

use crate::boot::RomDfu;
use crate::project::{
    Confirmed, DeleteTarget, NeedsConfirm, OverwriteTarget, PartSource, Pending, Project,
    ProjectSource, Target, TemplateCrc, Witnessed,
};
use crate::ui::hold::Presses;

use super::prompt::{
    Answer, Answers, Choice, ClearAnswer, DeleteAnswer, DfuAnswer, LoadAnswer, NameExistsAnswer,
    ReloadAnswer, ReplaceAnswer, SaveOverAnswer,
};
use said::Said;

/// A prompt with a pill that commits what it asked about, a `Confirms`.
pub trait Commits: Answers {
    type Confirms: Witnessed;
    const CONFIRM: Self;
}

/// A prompt that guards a replace: its confirming pill, and which saves first.
pub trait Guarded: Commits {
    const SAVE_FIRST: Self;
}

macro_rules! commits {
    ($($a:ident => $v:ident, $t:ty);+ $(;)?) => {
        $(impl Commits for $a {
            type Confirms = $t;
            const CONFIRM: Self = $a::$v;
        })+
    };
}

commits!(
    ReplaceAnswer => Replace, PartSource;
    ReloadAnswer => Reload, PartSource;
    LoadAnswer => LoadAnyway, ProjectSource;
    DeleteAnswer => Delete, DeleteTarget;
    ClearAnswer => Clear, OverwriteTarget;
    SaveOverAnswer => SaveOver, OverwriteTarget;
    NameExistsAnswer => Overwrite, OverwriteTarget;
    DfuAnswer => EnterDfu, RomDfu;
);

impl Guarded for ReplaceAnswer {
    const SAVE_FIRST: Self = ReplaceAnswer::SavePartFirst;
}

impl Guarded for ReloadAnswer {
    const SAVE_FIRST: Self = ReloadAnswer::SavePartFirst;
}

impl Guarded for LoadAnswer {
    const SAVE_FIRST: Self = LoadAnswer::SaveThenLoad;
}

/// The yes in `a`, if SEQ took the confirming pill.
pub fn said<A: Commits>(a: Answer<A>) -> Option<Said<A::Confirms>> {
    (a.picked() == Some(A::CONFIRM)).then(Said::new)
}

/// A replace the guard asked about, and its prompt's pick.
#[derive(Debug)]
pub struct Asked<R, A> {
    pending: Pending<R>,
    choice: Choice<A>,
}

/// SAVE PART FIRST / REPLACE / CANCEL.
pub type PartAsk = Asked<PartSource, ReplaceAnswer>;
/// SAVE PART FIRST / RELOAD / CANCEL.
pub type ReloadAsk = Asked<PartSource, ReloadAnswer>;
/// SAVE THEN LOAD / LOAD ANYWAY / CANCEL.
pub type ProjectAsk = Asked<ProjectSource, LoadAnswer>;

/// What the answer leaves.
#[derive(Debug)]
pub enum Reply<R: Target, A> {
    /// The replace waits on a save.
    SaveFirst(Asked<R, A>),
    Confirmed(Confirmed<R>),
    Cancel,
}

impl<R: Target, A: Guarded<Confirms = R>> Asked<R, A> {
    pub fn new(n: NeedsConfirm<R>) -> Self {
        Asked {
            pending: n.into_pending(),
            choice: Choice::new(),
        }
    }

    pub fn source(&self) -> R {
        self.pending.source()
    }

    pub fn choice(&self) -> &Choice<A> {
        &self.choice
    }

    /// A frame's keys: still open, or answered against the target as it
    /// is now.
    pub fn input(
        mut self,
        c: &impl Controls,
        p: &Presses,
        project: &Project,
    ) -> Result<Reply<R, A>, Self> {
        let Some(a) = self.choice.input(c, p) else {
            return Err(self);
        };
        if let Some(yes) = said(a) {
            return Ok(Reply::Confirmed(self.pending.confirm(project, yes)));
        }
        Ok(match a.picked() {
            Some(x) if x == A::SAVE_FIRST => Reply::SaveFirst(Asked {
                choice: Choice::new(),
                ..self
            }),
            _ => Reply::Cancel,
        })
    }

    /// After the save: through the guard again, or asked again.
    pub fn save_then(self, p: &Project, t: TemplateCrc) -> Result<Confirmed<R>, Self> {
        self.pending.save_then(p, t).map_err(Self::new)
    }

    /// The source alone, for a save the replace waits on: it confirms only
    /// through the guard.
    pub fn into_pending(self) -> Pending<R> {
        self.pending
    }
}
