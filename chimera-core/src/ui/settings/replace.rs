//! A replace waiting on its prompt (#258): the prompt's `Pending` and its
//! `Choice` together, so only a SEQ tap on the confirming pill makes the
//! `Confirmed`. Nothing else holds a `Said`:
//!
//! ```compile_fail,E0423
//! use chimera_core::ui::settings::replace::said::Said;
//! let _ = Said(());
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

use crate::project::{
    Confirmed, NeedsConfirm, PartSource, Pending, Project, ProjectSource, Target, TemplateCrc,
};
use crate::ui::hold::Presses;

use super::prompt::{Answers, Choice, LoadAnswer, ReloadAnswer, ReplaceAnswer};
use said::Said;

/// A prompt that guards a replace: which pill confirms it, which saves first.
pub trait Guarded: Answers {
    const CONFIRM: Self;
    const SAVE_FIRST: Self;
}

impl Guarded for ReplaceAnswer {
    const CONFIRM: Self = ReplaceAnswer::Replace;
    const SAVE_FIRST: Self = ReplaceAnswer::SavePartFirst;
}

impl Guarded for ReloadAnswer {
    const CONFIRM: Self = ReloadAnswer::Reload;
    const SAVE_FIRST: Self = ReloadAnswer::SavePartFirst;
}

impl Guarded for LoadAnswer {
    const CONFIRM: Self = LoadAnswer::LoadAnyway;
    const SAVE_FIRST: Self = LoadAnswer::SaveThenLoad;
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

impl<R: Target, A: Guarded> Asked<R, A> {
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
        Ok(match a.picked() {
            Some(x) if x == A::CONFIRM => {
                Reply::Confirmed(self.pending.confirm(project, Said::new()))
            }
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
