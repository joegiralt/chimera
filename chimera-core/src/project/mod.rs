//! A project: its name, its pool of Sounds and the Parts that play them
//! (projects spec § Model). The fields are private, so only this module
//! sets an `Origin` or moves a slot's generation.

mod codec;
mod guard;
mod ids;
mod marks;
mod note;
mod parts;
mod pool;
mod store;
mod swap;
mod template;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use codec::{ProjectCheck, ProjectDecoder, encode_new_project, encode_project, project_crc};
pub use guard::{
    Confirmed, DeleteTarget, NeedsConfirm, OverwriteTarget, Pending, ProjectSource, Prompt,
    ReplaceGuard, Seen, Target, Witnessed,
};
pub use ids::{PartId, PartSet, SlotId};
pub use marks::{
    ActionGone, PartAction, PartActionKind, PartActions, PartStatus, ProjectStatus, StatusCache,
    part_actions, part_status, project_status,
};
pub use note::{Differ, LINE_LEN, Line, ProjectNote, Subject};
pub use parts::{Origin, Part, PartEdit, PartRead, Performance, part_block, part_block_mut};
pub use pool::Pool;
pub use store::{
    FreshFile, ListOutcome, LoadOutcome, ProjectEntry, SaveTo, boot_project, clear_project,
    delete_project, list_projects, load_project, new_project_id, project_file, save_project,
};
pub use swap::{GateStep, LOAD_ACK_TIMEOUT_MS, LOAD_LINK, LoadGate, LoadLink, Settled, Swap};
pub use template::TemplateCrc;

use crate::dsp::fx_bus::FxParams;
use crate::name::ProjectName;
use crate::params::EngineType;
use crate::preset::Sound;
pub use crate::storage::ProjectFile;
use crate::storage::{ProjectId, sound_crc};

pub const NEW_NAME: &str = "NEW PROJECT";
const NEW_PROJECT_NAME: ProjectName = match ProjectName::new(NEW_NAME) {
    Ok(n) => n,
    Err(_) => panic!("NEW_NAME is a valid name"),
};

/// What a project is besides its Sounds and Parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProjectMeta {
    /// Set by a save or a file load.
    file: Option<ProjectFile>,
    name: ProjectName,
    /// The canonical CRC at the last save or load.
    saved_crc: Option<u32>,
}

impl ProjectMeta {
    fn new_project() -> Self {
        ProjectMeta {
            file: None,
            name: NEW_PROJECT_NAME,
            saved_crc: None,
        }
    }

    pub fn id(&self) -> Option<ProjectId> {
        self.file.map(ProjectFile::id)
    }

    pub fn file(&self) -> Option<ProjectFile> {
        self.file
    }

    pub fn name(&self) -> ProjectName {
        self.name
    }

    pub fn saved_crc(&self) -> Option<u32> {
        self.saved_crc
    }
}

/// What a Part is loaded from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartFrom {
    Slot(SlotId),
    Init(EngineType),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartSource {
    pub part: PartId,
    pub from: PartFrom,
}

/// The slot's users: a slot a Part plays isn't cleared.
#[derive(Debug, PartialEq)]
pub struct InUse(pub PartSet);

#[derive(Debug, PartialEq)]
pub enum ReplaceError {
    SlotEmpty,
    /// The target moved since the replace was confirmed.
    Changed,
}

pub struct Project {
    meta: ProjectMeta,
    pool: Pool,
    perf: Performance,
    /// Moves on every `&mut` path, so a status keyed on it is never stale.
    rev: u32,
}

crate::in_place::field_list!(Project => Project { meta, pool, perf, rev });

impl Project {
    /// Wrapping; any two reads with an edit between differ.
    pub fn rev(&self) -> u32 {
        self.rev
    }

    /// Every `&mut` path calls this first: nothing reads between it and the edit.
    fn bump(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    pub fn meta(&self) -> &ProjectMeta {
        &self.meta
    }

    pub fn perf(&self) -> &Performance {
        &self.perf
    }

    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    pub fn part(&self, p: PartId) -> &Part {
        self.perf.part(p)
    }

    /// A Part read as the pages read it; can't be written through.
    pub fn read_part(&self, p: PartId) -> PartRead<'_> {
        let x = self.part(p);
        PartRead {
            sound: &x.sound,
            mix: &x.mix,
            fx: &self.perf.fx,
        }
    }

    pub fn edit_part(&mut self, p: PartId) -> PartEdit<'_> {
        self.bump();
        self.perf.edit(p)
    }

    pub fn edit_fx(&mut self) -> &mut FxParams {
        self.bump();
        &mut self.perf.fx
    }

    pub fn set_name(&mut self, n: ProjectName) {
        self.bump();
        self.meta.name = n;
    }

    /// The Parts whose Origin is slot `s`.
    pub fn users(&self, s: SlotId) -> PartSet {
        PartId::ALL
            .into_iter()
            .filter(|&p| matches!(self.part(p).origin, Origin::Slot { slot, .. } if slot == s))
            .fold(PartSet::EMPTY, PartSet::with)
    }

    pub fn pool_store(&mut self, s: SlotId, sound: Sound) {
        self.bump();
        self.pool.store(s, sound);
    }

    pub fn pool_clear(&mut self, s: SlotId) -> Result<(), InUse> {
        self.bump();
        let users = self.users(s);
        if !users.is_empty() {
            return Err(InUse(users));
        }
        self.pool.clear(s);
        Ok(())
    }

    /// Stores a copy of p's sound in s and sets p's Origin fresh. Returns the
    /// other users of s that now derive Stale(s). Only a Part action
    /// (`apply_part_action`) saves a Part (#257):
    ///
    /// ```compile_fail,E0624
    /// use chimera_core::project::{PartId, Project, SlotId};
    /// fn f(p: &mut Project) {
    ///     let _ = p.save_part_to(PartId::ALL[0], SlotId::ALL[0]);
    /// }
    /// ```
    pub(crate) fn save_part_to(&mut self, p: PartId, s: SlotId) -> PartSet {
        self.bump();
        let others = self.users(s);
        let sound = self.part(p).sound.clone();
        let crc = sound_crc(&sound);
        self.pool.store(s, sound);
        self.perf.parts[p.index()].origin = Origin::Slot {
            slot: s,
            generation: self.pool.generation(s),
            crc,
        };
        others
            .iter()
            .filter(|&o| o != p && part_status(self.part(o), &self.pool) == PartStatus::Stale(s))
            .fold(PartSet::EMPTY, PartSet::with)
    }

    /// Replaces the Part's Sound; its mix stays. Refused if the Part moved
    /// since it was confirmed, or the slot emptied.
    pub fn replace_part(&mut self, c: Confirmed<PartSource>) -> Result<(), ReplaceError> {
        if !c.holds(self) {
            return Err(ReplaceError::Changed);
        }
        self.load_part(c.target())
    }

    /// Only through `replace_part` or a Part action.
    fn load_part(&mut self, src: PartSource) -> Result<(), ReplaceError> {
        self.bump();
        let part = &mut self.perf.parts[src.part.index()];
        match src.from {
            PartFrom::Slot(s) => {
                let sound = self.pool.get(s).ok_or(ReplaceError::SlotEmpty)?;
                part.sound.clone_from(sound);
                part.origin = Origin::Slot {
                    slot: s,
                    generation: self.pool.generation(s),
                    crc: sound_crc(sound),
                };
            }
            PartFrom::Init(e) => {
                part.sound = Sound::init(e);
                part.origin = Origin::Init(e);
            }
        }
        Ok(())
    }
}
