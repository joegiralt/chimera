//! The one guard on every replace that can lose work (projects spec § Types
//! decide what's possible): a slot into an `Edited` Part, or `+ NEW` or a
//! project load over a `Modified` project. Only a `Confirmed` replaces, and
//! it holds a witness of what it confirmed, so a replace is refused once its
//! target moved.
//!
//! A `Confirmed` is made here only:
//!
//! ```compile_fail,E0451
//! use chimera_engine::project::{Confirmed, PartSource};
//! fn forge(target: PartSource) -> Confirmed<PartSource> {
//!     Confirmed { target, witness: 0 }
//! }
//! ```
//!
//! A project's confirmation doesn't replace a Part:
//!
//! ```compile_fail,E0308
//! use chimera_engine::project::{Project, ProjectSource, ReplaceGuard, TemplateCrc};
//! fn f(p: &mut Project, t: TemplateCrc) {
//!     let c = ReplaceGuard::check(p, t, ProjectSource::New).unwrap();
//!     let _ = p.replace_part(c);
//! }
//! ```
//!
//! and nothing loads a Part around the guard:
//!
//! ```compile_fail,E0624
//! use chimera_engine::project::{PartSource, Project};
//! fn f(p: &mut Project, s: PartSource) {
//!     let _ = p.load_part(s);
//! }
//! ```
//!
//! The same calls through the guard:
//!
//! ```
//! use chimera_engine::project::{PartSource, Project, ReplaceGuard, TemplateCrc};
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
use crate::orbit::scene::{OrbitSong, SceneIdx};
use crate::storage::{Crc32, Generation, ProjectId, sound_crc};

use super::marks::status_at;
use super::{
    KitDoor, LibraryPart, Origin, PartFrom, PartSource, Project, ProjectEntry, ProjectFile,
    ProjectStatus, SoundStatus, TemplateCrc, project_crc, project_status, sound_status,
};
use crate::confirm::Said;

/// The question asked before a replace that would lose work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// SAVE PART FIRST / REPLACE / CANCEL.
    SavePartFirst,
    /// SAVE THEN LOAD / LOAD ANYWAY / CANCEL.
    SaveProjectFirst,
    /// REPLACE / CANCEL: a library load of a DRUM Sound over a kit in no
    /// library file (ADR 0305). No save first: a slot holds no kit.
    KitUnsaved,
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::PartSource {}
    impl Sealed for super::ProjectSource {}
    impl Sealed for super::LibraryPart {}
    impl Sealed for super::LibraryKit {}
    impl Sealed for super::OverwriteTarget {}
    impl Sealed for super::DeleteTarget {}
    impl Sealed for super::SceneSlot {}
    impl Sealed for crate::boot::RomDfu {}
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
    #[optimize(size)]
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

/// A Part whose Sound is `Edited` asks; a `Clean` or `Stale` one
/// doesn't. Its kit is not weighed: a slot or INIT load, a reload and a
/// program change leave the kit on the Part (ADR 0265, 0305).
impl Target for PartSource {
    #[optimize(size)]
    fn at_risk(&self, p: &Project, _: TemplateCrc) -> Option<Prompt> {
        (sound_status(p.part(self.part), p.pool()) == SoundStatus::Edited)
            .then_some(Prompt::SavePartFirst)
    }

    /// The Part's `sound_crc`, its Origin and, for a slot, the slot's
    /// generation now: `sound_status` reads only these and the slot's
    /// contents, which move its generation.
    #[optimize(size)]
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

impl LibraryPart {
    /// The Part as a replace sees it: only the Part is witnessed.
    #[optimize(size)]
    fn as_part(&self) -> PartSource {
        PartSource {
            part: self.part,
            from: PartFrom::Init(crate::params::EngineType::Algo),
        }
    }
}

impl Witnessed for LibraryPart {
    type Witness = u32;
}

/// A library load's first question, the Sound's, as `PartSource`'s: the
/// file is checked by the load, not the guard. Its confirmation loads
/// nothing: the kit's question is next (`ReplaceGuard::kit`).
impl Target for LibraryPart {
    #[optimize(size)]
    fn at_risk(&self, p: &Project, t: TemplateCrc) -> Option<Prompt> {
        self.as_part().at_risk(p, t)
    }

    #[optimize(size)]
    fn witness(&self, p: &Project) -> u32 {
        self.as_part().witness(p)
    }
}

/// A library load whose Sound question is settled: the kit's turn (ADR
/// 0305). A DRUM Sound's load replaces the Part's kit, the one thing
/// that does, so only here is the kit weighed. Made only by
/// `ReplaceGuard::kit`, from the Sound's confirmation:
///
/// ```compile_fail,E0451
/// use chimera_engine::project::{LibraryKit, LibraryPart};
/// fn forge(lib: LibraryPart) -> LibraryKit {
///     LibraryKit { lib, sound: 0 }
/// }
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LibraryKit {
    lib: LibraryPart,
    /// The Part's witness as the Sound's question confirmed it.
    sound: u32,
}

impl LibraryKit {
    #[optimize(size)]
    pub fn part(&self) -> super::PartId {
        self.lib.part
    }

    #[optimize(size)]
    pub fn file(&self) -> super::LibFile {
        self.lib.file
    }
}

impl Witnessed for LibraryKit {
    type Witness = u32;
}

/// The Part's kit is at risk when the file may be a DRUM Sound's (its
/// listed engine DRUM, or unknown) and the kit is neither the default
/// nor one the library holds: whatever engine the Part plays now (a kit
/// kept through a switch away from DRUM counts) and whatever loaded it.
impl Target for LibraryKit {
    #[optimize(size)]
    fn at_risk(&self, p: &Project, _: TemplateCrc) -> Option<Prompt> {
        let drum = matches!(
            self.lib.file.engine,
            None | Some(crate::params::EngineType::Drum)
        );
        (drum && KitDoor::seen(p, self.lib.part).unsaved()).then_some(Prompt::KitUnsaved)
    }

    /// The Part's own witness now, then its kit as seen: its place and
    /// CRC, in every engine state.
    #[optimize(size)]
    fn witness(&self, p: &Project) -> u32 {
        let mut c = Crc32::new();
        c.update(&self.lib.as_part().witness(p).to_le_bytes());
        c.update(&KitDoor::seen(p, self.lib.part).bytes());
        c.finish()
    }
}

impl Confirmed<LibraryKit> {
    /// The Part and its kit still what both questions confirmed: the
    /// kit's (`holds`), and the Sound's before it.
    #[optimize(size)]
    pub fn still(&self, p: &Project) -> bool {
        self.holds(p) && self.target.lib.as_part().witness(p) == self.target.sound
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
    #[optimize(size)]
    fn at_risk(&self, p: &Project, t: TemplateCrc) -> Option<Prompt> {
        (project_status(p, t) == ProjectStatus::Modified).then_some(Prompt::SaveProjectFirst)
    }

    /// The whole project is the target, whichever source replaces it.
    #[optimize(size)]
    fn witness(&self, p: &Project) -> u32 {
        project_crc(p)
    }

    /// One hash for both.
    #[optimize(size)]
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
    #[optimize(size)]
    pub fn target(&self) -> R {
        self.target
    }

    #[optimize(size)]
    pub(crate) fn witness(&self) -> R::Witness {
        self.witness
    }
}

impl<R: Target> Confirmed<R> {
    #[optimize(size)]
    fn now(target: R, p: &Project) -> Self {
        Confirmed {
            target,
            witness: target.witness(p),
        }
    }

    /// The target's state still what was confirmed.
    #[optimize(size)]
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
    #[optimize(size)]
    pub fn prompt(&self) -> Prompt {
        self.prompt
    }

    #[optimize(size)]
    pub fn into_pending(self) -> Pending<R> {
        self.pending
    }
}

/// A replace waiting on its prompt: the source's id, never parsed data.
/// CANCEL drops it.
#[derive(Debug)]
pub struct Pending<R>(R);

impl<R: Target> Pending<R> {
    #[optimize(size)]
    pub fn source(&self) -> R {
        self.0
    }

    /// After the caller's save: asks again if it was cancelled or failed.
    #[optimize(size)]
    pub fn save_then(self, p: &Project, t: TemplateCrc) -> Result<Confirmed<R>, NeedsConfirm<R>> {
        ReplaceGuard::check(p, t, self.0)
    }
}

impl<R: Target> Pending<R> {
    /// REPLACE / LOAD ANYWAY, answered: the target as it is now is what's
    /// confirmed. Only a prompt's confirming pill holds a `Said` (#258).
    #[optimize(size)]
    pub fn confirm(self, p: &Project, _: Said<R>) -> Confirmed<R> {
        Confirmed::now(self.0, p)
    }

    /// Confirmed without a prompt: tests only.
    #[cfg(any(test, feature = "test-support"))]
    #[optimize(size)]
    pub fn anyway(self, p: &Project) -> Confirmed<R> {
        Confirmed::now(self.0, p)
    }
}

pub struct ReplaceGuard;

impl ReplaceGuard {
    /// A library load's second question, after its Sound's: the kit's.
    #[optimize(size)]
    pub fn kit(
        p: &Project,
        t: TemplateCrc,
        c: Confirmed<LibraryPart>,
    ) -> Result<Confirmed<LibraryKit>, NeedsConfirm<LibraryKit>> {
        let lib = c.target;
        Self::check(
            p,
            t,
            LibraryKit {
                lib,
                sound: c.witness,
            },
        )
    }

    #[optimize(size)]
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
/// use chimera_engine::project::{OverwriteTarget, ProjectFile};
/// fn forge(file: ProjectFile) -> OverwriteTarget {
///     OverwriteTarget { file }
/// }
/// ```
///
/// and only a prompt's yes confirms one:
///
/// ```compile_fail,E0061
/// use chimera_engine::project::{Confirmed, OverwriteTarget, ProjectEntry};
/// fn forge(e: &ProjectEntry) -> Confirmed<OverwriteTarget> {
///     Confirmed::<OverwriteTarget>::answered(e)
/// }
/// ```
///
/// and a DELETE's yes is no overwrite's:
///
/// ```compile_fail,E0308
/// use chimera_engine::project::{Confirmed, DeleteTarget, OverwriteTarget, ProjectEntry};
/// use chimera_engine::confirm::Said;
/// fn forge(e: &ProjectEntry, yes: Said<DeleteTarget>) -> Confirmed<OverwriteTarget> {
///     Confirmed::<OverwriteTarget>::answered(e, yes)
/// }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverwriteTarget {
    file: ProjectFile,
}

/// A file DELETE removes. Only `answered` confirms one, with a prompt's
/// yes:
///
/// ```compile_fail,E0061
/// use chimera_engine::project::{Confirmed, DeleteTarget, ProjectEntry};
/// fn forge(e: &ProjectEntry) -> Confirmed<DeleteTarget> {
///     Confirmed::<DeleteTarget>::answered(e)
/// }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeleteTarget {
    file: ProjectFile,
}

impl OverwriteTarget {
    #[optimize(size)]
    pub fn file(self) -> ProjectFile {
        self.file
    }
}

impl DeleteTarget {
    #[optimize(size)]
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
    #[optimize(size)]
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

/// Entering DFU loses nothing on the card: nothing to witness.
impl Witnessed for crate::boot::RomDfu {
    type Witness = ();
}

macro_rules! answered {
    ($($t:ident: $what:literal),+) => {
        $(impl Confirmed<$t> {
            #[doc = concat!($what, " answered on the listed `e`: only a prompt's")]
            /// confirming pill holds a `Said`.
            #[optimize(size)]
            pub fn answered(e: &ProjectEntry, _: Said<$t>) -> Self {
                Self::listed(e)
            }

            /// Confirmed without a prompt: tests only.
            #[cfg(any(test, feature = "test-support"))]
            #[optimize(size)]
            pub(crate) fn unasked(e: &ProjectEntry) -> Self {
                Self::listed(e)
            }

            #[optimize(size)]
            fn listed(e: &ProjectEntry) -> Self {
                Confirmed {
                    target: $t { file: e.file() },
                    witness: Seen::of(e),
                }
            }
        })+
    };
}

answered!(OverwriteTarget: "SAVE OVER, CLEAR or OVERWRITE THAT ONE", DeleteTarget: "DELETE");

/// A scene slot SCN's SAVE OVER writes. Only a prompt's yes confirms
/// one, with what the slot was when the prompt opened:
///
/// ```compile_fail,E0451
/// use chimera_engine::project::{Confirmed, SceneAsk, SceneSlot};
/// fn forge(a: &SceneAsk) -> Confirmed<SceneSlot> {
///     Confirmed { target: a.slot(), witness: 0 }
/// }
/// ```
///
/// ```compile_fail,E0423
/// use chimera_engine::orbit::scene::SceneIdx;
/// use chimera_engine::project::SceneSlot;
/// fn forge(n: SceneIdx) -> SceneSlot {
///     SceneSlot(n)
/// }
/// ```
///
/// ```compile_fail,E0061
/// use chimera_engine::project::{Confirmed, SceneAsk, SceneSlot};
/// fn forge(a: SceneAsk) -> Confirmed<SceneSlot> {
///     Confirmed::<SceneSlot>::said(a)
/// }
/// ```
///
/// and a project's SAVE OVER is no scene's:
///
/// ```compile_fail,E0308
/// use chimera_engine::project::{Confirmed, OverwriteTarget, SceneAsk, SceneSlot};
/// use chimera_engine::confirm::Said;
/// fn forge(a: SceneAsk, yes: Said<OverwriteTarget>) -> Confirmed<SceneSlot> {
///     Confirmed::<SceneSlot>::said(a, yes)
/// }
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneSlot(SceneIdx);

impl SceneSlot {
    #[optimize(size)]
    pub fn index(self) -> SceneIdx {
        self.0
    }
}

/// A slot written since refuses it: the slot's rev as the prompt saw it.
impl Witnessed for SceneSlot {
    type Witness = u16;
}

/// SAVE OVER's question: the slot, as it was when the prompt opened.
#[derive(Debug)]
pub struct SceneAsk {
    slot: SceneSlot,
    seen: u16,
}

impl SceneAsk {
    /// Slot `n` of `song` now.
    #[optimize(size)]
    pub fn of(song: &OrbitSong, n: SceneIdx) -> SceneAsk {
        SceneAsk {
            slot: SceneSlot(n),
            seen: song.slot_rev(n),
        }
    }

    #[optimize(size)]
    pub fn slot(&self) -> SceneSlot {
        self.slot
    }
}

impl Confirmed<SceneSlot> {
    /// SAVE OVER answered: only a prompt's confirming pill holds a `Said`.
    /// It confirms the slot as the prompt saw it, not as it is now.
    #[optimize(size)]
    pub fn said(a: SceneAsk, _: Said<SceneSlot>) -> Self {
        Confirmed {
            target: a.slot,
            witness: a.seen,
        }
    }

    /// The slot unwritten since it was confirmed.
    #[optimize(size)]
    pub fn holds(&self, song: &OrbitSong) -> bool {
        song.slot_rev(self.target.0) == self.witness
    }

    /// Slot `n` confirmed now, without a prompt: tests only.
    #[cfg(any(test, feature = "test-support"))]
    #[optimize(size)]
    pub fn unasked(song: &OrbitSong, n: SceneIdx) -> Self {
        Confirmed {
            target: SceneSlot(n),
            witness: song.slot_rev(n),
        }
    }
}
