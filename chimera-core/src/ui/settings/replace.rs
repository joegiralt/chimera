//! A Part's replace waiting on its prompt (#258): the UI holds a `PartAsk`,
//! never a bare `Pending`, and only REPLACE answered makes the
//! `Confirmed`. Nothing else holds a `Said`:
//!
//! ```compile_fail,E0423
//! use chimera_core::ui::settings::replace::Said;
//! let _ = Said(());
//! ```
//!
//! ```compile_fail,E0423
//! use chimera_core::project::{PartSource, Pending};
//! use chimera_core::ui::settings::replace::PartAsk;
//! fn forge(p: Pending<PartSource>) -> PartAsk {
//!     PartAsk(p)
//! }
//! ```

use crate::project::{Confirmed, NeedsConfirm, PartSource, Pending, Project, TemplateCrc};

use super::prompt::{Answer, ReplaceAnswer};

/// What `Pending::<PartSource>::replace` takes: the prompt's REPLACE.
pub struct Said(());

/// A replace into an Edited Part, asked.
#[derive(Debug)]
pub struct PartAsk(Pending<PartSource>);

/// What the prompt's answer leaves.
#[derive(Debug)]
pub enum PartReply {
    /// SAVE PART FIRST: the replace waits on the save.
    SaveFirst(PartAsk),
    Replace(Confirmed<PartSource>),
    Cancel,
}

impl PartAsk {
    pub fn new(n: NeedsConfirm<PartSource>) -> Self {
        PartAsk(n.into_pending())
    }

    pub fn source(&self) -> PartSource {
        self.0.source()
    }

    /// `a`, against the Part as it is now.
    pub(crate) fn answer(self, a: Answer<ReplaceAnswer>, p: &Project) -> PartReply {
        match a {
            Answer::Pick(ReplaceAnswer::Replace) => PartReply::Replace(self.0.replace(p, Said(()))),
            Answer::Pick(ReplaceAnswer::SavePartFirst) => PartReply::SaveFirst(self),
            Answer::Pick(ReplaceAnswer::Cancel) | Answer::Cancel => PartReply::Cancel,
        }
    }

    /// After the save: through the guard again, or asked again.
    pub fn save_then(self, p: &Project, t: TemplateCrc) -> Result<Confirmed<PartSource>, PartAsk> {
        self.0.save_then(p, t).map_err(PartAsk::new)
    }
}
