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

use chimera_hal::store::VolumeId;

use crate::block::DiskCode;
use crate::storage::{Crc32, ProjectId, sound_crc};

use super::marks::status_at;
use super::{
    Origin, PartSource, PartStatus, Project, ProjectStatus, TemplateCrc, part_status, project_crc,
    project_status,
};

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
}

/// What a replace overwrites. Implemented for the two targets only.
pub trait Target: Copy + sealed::Sealed {
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
pub struct Confirmed<R> {
    target: R,
    witness: u32,
}

impl<R: Target> Confirmed<R> {
    fn now(target: R, p: &Project) -> Self {
        Confirmed {
            target,
            witness: target.witness(p),
        }
    }

    pub fn target(&self) -> R {
        self.target
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

    /// REPLACE / LOAD ANYWAY: the target as it is now is what's confirmed.
    pub fn anyway(self, p: &Project) -> Confirmed<R> {
        Confirmed::now(self.0, p)
    }

    /// After the caller's save: asks again if it was cancelled or failed.
    pub fn save_then(self, p: &Project, t: TemplateCrc) -> Result<Confirmed<R>, NeedsConfirm<R>> {
        ReplaceGuard::check(p, t, self.0)
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
