//! A replace waiting on its prompt (#258): the prompt's `Pending` and its
//! `Choice` together, so only an EDIT press on the confirming pill makes the
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

/// The prompt's yes (`chimera_engine::confirm`).
pub mod said {
    pub use chimera_engine::confirm::Said;
}

use chimera_hal::Controls;

use crate::project::{
    Confirmed, LibraryPart, NeedsConfirm, PartSource, Pending, Project, ProjectSource, Target,
    TemplateCrc,
};
use crate::ui::hold::Presses;
use crate::ui::list_turn;

use super::prompt::{Choice, LibraryAnswer, LoadAnswer, ReloadAnswer, ReplaceAnswer};

pub use chimera_engine::confirm::{Commits, Guarded, said};

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
/// A library Sound: SAVE PART FIRST / REPLACE / CANCEL.
pub type LibraryAsk = Asked<LibraryPart, LibraryAnswer>;
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
        let Some(a) = self.choice.input(i32::from(list_turn(c, &[])), p) else {
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
