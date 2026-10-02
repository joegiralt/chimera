//! The one guard on every replace that can lose work (projects spec § Types
//! decide what's possible): a slot into an `Edited` Part, or `+ NEW` or a
//! project load over a `Modified` project. Only a `Confirmed` replaces, and
//! it holds a witness of what it confirmed, so a replace is refused once its
//! target moved.
//!
//! A `Confirmed` is made here only:
//!
//! ```compile_fail,E0451
//! use chimera_core::project::{Confirmed, PartSource};
//! fn forge(target: PartSource) -> Confirmed<PartSource> {
//!     Confirmed { target, witness: 0 }
//! }
//! ```
//!
//! A project's confirmation doesn't replace a Part:
//!
//! ```compile_fail,E0308
//! use chimera_core::project::{Project, ProjectSource, ReplaceGuard, TemplateCrc};
//! fn f(p: &mut Project, t: TemplateCrc) {
//!     let c = ReplaceGuard::check(p, t, ProjectSource::New).unwrap();
//!     let _ = p.replace_part(c);
//! }
//! ```
//!
//! and nothing loads a Part around the guard:
//!
//! ```compile_fail,E0624
//! use chimera_core::project::{PartSource, Project};
//! fn f(p: &mut Project, s: PartSource) {
//!     let _ = p.load_part(s);
//! }
//! ```
//!
//! The same calls through the guard:
//!
//! ```
//! use chimera_core::project::{PartSource, Project, ReplaceGuard, TemplateCrc};
//! fn f(p: &mut Project, t: TemplateCrc, s: PartSource) {
//!     if let Ok(c) = ReplaceGuard::check(p, t, s) {
//!         let _ = p.replace_part(c);
//!     }
//! }
//! ```

use core::fmt::Debug;

use chimera_hal::store::VolumeId;

use crate::block::DiskCode;
use crate::name::ProjectName;
use crate::storage::{Crc32, Generation, ProjectId, sound_crc};

use super::marks::status_at;
use super::{
    Origin, PartSource, PartStatus, Project, ProjectEntry, ProjectFile, ProjectStatus, TemplateCrc,
    part_status, project_crc, project_status,
};
use crate::ui::settings::replace::said::Said;

/// The question asked before a replace that would lose work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// SAVE PART FIRST / REPLACE / CANCEL.
    SavePartFirst,
    /// SAVE THEN LOAD / LOAD ANYWAY / CANCEL.
    SaveProjectFirst,
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::PartSource {}
    impl Sealed for super::ProjectSource {}
    impl Sealed for super::OverwriteTarget {}
    impl Sealed for super::DeleteTarget {}
}

/// What a `Confirmed` can confirm, and what it keeps of the target then.
pub trait Witnessed: Copy + sealed::Sealed {
    type Witness: Copy + PartialEq + Debug;
}

/// What a replace overwrites. Implemented for the two targets only.
pub trait Target: Witnessed<Witness = u32> {
    /// The prompt the replace needs now, if any.
    fn at_risk(&self, p: &Project, t: TemplateCrc) -> Option<Prompt>;
    /// The target's state, as a confirmation saw it: every input to
    /// `at_risk` but the template (a wrapped generation may alias).
    fn witness(&self, p: &Project) -> u32;
    /// The witness when nothing is at risk, else the prompt.
    fn assess(&self, p: &Project, t: TemplateCrc) -> Result<u32, Prompt> {
        match self.at_risk(p, t) {
            Some(prompt) => Err(prompt),
            None => Ok(self.witness(p)),
        }
    }
}

impl Witnessed for PartSource {
    type Witness = u32;
}

/// An `Edited` Part asks; a `Clean` or `Stale` one doesn't.
impl Target for PartSource {
    fn at_risk(&self, p: &Project, _: TemplateCrc) -> Option<Prompt> {
        (part_status(p.part(self.part), p.pool()) == PartStatus::Edited)
            .then_some(Prompt::SavePartFirst)
    }

    /// The Part's `sound_crc`, its Origin and, for a slot, the slot's
    /// generation now: `part_status` reads only these and the slot's
    /// contents, which move its generation.
    fn witness(&self, p: &Project) -> u32 {
        let part = p.part(self.part);
        let mut c = Crc32::new();
        c.update(&sound_crc(&part.sound).to_le_bytes());
        match part.origin {
            Origin::Slot {
                slot,
                generation,
                crc,
            } => {
                c.update(&[0, slot.index() as u8]);
                c.update(&generation.to_le_bytes());
                c.update(&crc.to_le_bytes());
                c.update(&p.pool().generation(slot).to_le_bytes());
            }
            Origin::Init(e) => c.update(&[1, e.disk_code()]),
        }
        c.finish()
    }
}

/// A project to load over the current one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectSource {
    /// `vol` is the card the file was listed on.
    File {
        id: ProjectId,
        vol: VolumeId,
    },
    New,
}

impl Witnessed for ProjectSource {
    type Witness = u32;
}

/// A `Modified` project asks; a `Pristine` or `Saved` one doesn't.
impl Target for ProjectSource {
    fn at_risk(&self, p: &Project, t: TemplateCrc) -> Option<Prompt> {
        (project_status(p, t) == ProjectStatus::Modified).then_some(Prompt::SaveProjectFirst)
    }

    /// The whole project is the target, whichever source replaces it.
    fn witness(&self, p: &Project) -> u32 {
        project_crc(p)
    }

    /// One hash for both.
    fn assess(&self, p: &Project, t: TemplateCrc) -> Result<u32, Prompt> {
        let crc = project_crc(p);
        match status_at(p, t, crc) {
            ProjectStatus::Modified => Err(Prompt::SaveProjectFirst),
            ProjectStatus::Pristine | ProjectStatus::Saved => Ok(crc),
        }
    }
}

/// A replace the guard let through, and the state of its target then.
#[must_use]
#[derive(Debug)]
pub struct Confirmed<R: Witnessed> {
    target: R,
    witness: R::Witness,
}

impl<R: Witnessed> Confirmed<R> {
    pub fn target(&self) -> R {
        self.target
    }

    pub(crate) fn witness(&self) -> R::Witness {
        self.witness
    }
}

impl<R: Target> Confirmed<R> {
    fn now(target: R, p: &Project) -> Self {
        Confirmed {
            target,
            witness: target.witness(p),
        }
    }

    /// The target's state still what was confirmed.
    pub fn holds(&self, p: &Project) -> bool {
        self.target.witness(p) == self.witness
    }
}

/// A replace that needs its prompt answered.
#[must_use]
#[derive(Debug)]
pub struct NeedsConfirm<R> {
    pending: Pending<R>,
    prompt: Prompt,
}

impl<R> NeedsConfirm<R> {
    pub fn prompt(&self) -> Prompt {
        self.prompt
    }

    pub fn into_pending(self) -> Pending<R> {
        self.pending
    }
}

/// A replace waiting on its prompt: the source's id, never parsed data.
/// CANCEL drops it.
#[derive(Debug)]
pub struct Pending<R>(R);

impl<R: Target> Pending<R> {
    pub fn source(&self) -> R {
        self.0
    }

    /// After the caller's save: asks again if it was cancelled or failed.
    pub fn save_then(self, p: &Project, t: TemplateCrc) -> Result<Confirmed<R>, NeedsConfirm<R>> {
        ReplaceGuard::check(p, t, self.0)
    }
}

impl<R: Target> Pending<R> {
    /// REPLACE / LOAD ANYWAY, answered: the target as it is now is what's
    /// confirmed. Only a prompt's confirming pill holds a `Said` (#258).
    pub fn confirm(self, p: &Project, _: Said) -> Confirmed<R> {
        Confirmed::now(self.0, p)
    }

    /// Confirmed without a prompt: tests only.
    #[cfg(any(test, feature = "test-support"))]
    pub fn anyway(self, p: &Project) -> Confirmed<R> {
        Confirmed::now(self.0, p)
    }
}

pub struct ReplaceGuard;

impl ReplaceGuard {
    pub fn check<R: Target>(
        p: &Project,
        t: TemplateCrc,
        r: R,
    ) -> Result<Confirmed<R>, NeedsConfirm<R>> {
        match r.assess(p, t) {
            Ok(witness) => Ok(Confirmed { target: r, witness }),
            Err(prompt) => Err(NeedsConfirm {
                pending: Pending(r),
                prompt,
            }),
        }
    }
}

/// A file a save writes over: SAVE OVER, or CLEAR. Only `answered`
/// makes one, from a listing:
///
/// ```compile_fail,E0451
/// use chimera_core::project::{OverwriteTarget, ProjectFile};
/// fn forge(file: ProjectFile) -> OverwriteTarget {
///     OverwriteTarget { file }
/// }
/// ```
///
/// ```compile_fail,E0624
/// use chimera_core::project::{Confirmed, OverwriteTarget, ProjectEntry};
/// fn forge(e: &ProjectEntry) -> Confirmed<OverwriteTarget> {
///     Confirmed::<OverwriteTarget>::answered(e)
/// }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverwriteTarget {
    file: ProjectFile,
}

/// A file DELETE removes. Only `answered` confirms one:
///
/// ```compile_fail,E0624
/// use chimera_core::project::{Confirmed, DeleteTarget, ProjectEntry};
/// fn forge(e: &ProjectEntry) -> Confirmed<DeleteTarget> {
///     Confirmed::<DeleteTarget>::answered(e)
/// }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeleteTarget {
    file: ProjectFile,
}

impl OverwriteTarget {
    pub fn file(self) -> ProjectFile {
        self.file
    }
}

impl DeleteTarget {
    pub fn file(self) -> ProjectFile {
        self.file
    }
}

/// A file's newest readable header as listed, from its header alone. The
/// name as well as the generation: a pair deleted and made again at the
/// same id starts its generations over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    generation: Option<Generation>,
    name: Option<ProjectName>,
}

impl Seen {
    pub(crate) fn of(e: &ProjectEntry) -> Self {
        Seen {
            generation: e.generation,
            name: e.name,
        }
    }
}

/// A file saved, cleared, deleted or made again since refuses it.
impl Witnessed for OverwriteTarget {
    type Witness = Seen;
}

impl Witnessed for DeleteTarget {
    type Witness = Seen;
}

impl Confirmed<OverwriteTarget> {
    /// SAVE OVER or CLEAR answered on the listed `e`.
    pub(crate) fn answered(e: &ProjectEntry) -> Self {
        Confirmed {
            target: OverwriteTarget { file: e.file() },
            witness: Seen::of(e),
        }
    }
}

impl Confirmed<DeleteTarget> {
    /// DELETE answered on the listed `e`.
    pub(crate) fn answered(e: &ProjectEntry) -> Self {
        Confirmed {
            target: DeleteTarget { file: e.file() },
            witness: Seen::of(e),
        }
    }
}
