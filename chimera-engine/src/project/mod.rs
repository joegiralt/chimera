//! A project: its name, its pool of Sounds and the Parts that play them
//! (projects spec § Model). The fields are private, so only this module
//! sets an `Origin` or moves a slot's generation.

pub mod arena;
mod codec;
mod guard;
mod ids;
mod kit_library;
mod kits;
mod library;
mod marks;
mod note;
mod parts;
mod pool;
mod sources;
mod store;
mod streamed;
mod swap;
mod template;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use codec::{ProjectCheck, ProjectDecoder, encode_new_project, encode_project, project_crc};
pub use guard::{
    Confirmed, DeleteTarget, LibraryKit, NeedsConfirm, OverwriteTarget, Pending, ProjectSource,
    Prompt, ReplaceGuard, SceneAsk, SceneSlot, Seen, Target, Witnessed,
};
pub(crate) use ids::bounded_index;
pub use ids::{PartId, PartSet, SlotId};
#[cfg(feature = "bank2")]
pub use kit_library::kit_door_ok;
pub use kit_library::{
    FileKit, KitCrc, KitDoor, KitFound, KitMark, KitPut, KitScan, KitSeen, KitState, ReadKit,
};
pub use kits::{FollowHold, Kits, PadLights};
pub use library::{
    AdoptError, Adopted, LibEntry, LibFile, LibNote, LibPage, LibRead, LibSeen, LibraryPart,
    PageAt, PartSave, ReadPart, SavedPart, Shelf, SourceNeed, Staged, list_library, peek_note,
    read_library_sound, save_library_part, save_library_sound,
};
pub use marks::{
    ActionRefused, Edits, PartAction, PartActionKind, PartActions, PartStatus, ProjectStatus,
    SoundStatus, StatusCache, kit_state, part_actions, part_status, project_status, sound_status,
};
pub use note::{Differ, LINE_LEN, Line, ProjectNote, Subject};
pub use parts::{Loads, Origin, Part, PartEdit, PartRead, Performance, part_block, part_block_mut};
pub use pool::{EncodedSound, Pool, PoolFull};
pub use sources::{
    Absence, Purged, SOURCE_HEAD, SourceEntry, SourceId, SourceRef, SourceSet, Unused, Users,
    source_users,
};
pub use store::PROJECT_DEMO_TOP;
#[cfg(feature = "card-put")]
pub use store::peek_entry;
pub use store::still_over;
pub use store::{
    CardOut, FreshFile, ListOutcome, LoadOutcome, MAX_LISTED, ProjectEntry, SaveTo, boot_project,
    clear_project, delete_project, find_named, list_projects, load_project, new_project_id,
    project_file, save_project, save_project_as,
};
pub use streamed::{LoadDoor, LoadKind, Refusal, StreamFail, StreamLoad, StreamStep, load_kind};
pub use swap::{GateStep, LOAD_ACK_TIMEOUT_MS, LOAD_LINK, LoadGate, LoadLink, Settled, Swap};
pub use template::{NEW_PARTS, TemplateCrc};

use crate::dsp::fx_bus::FxParams;
use crate::hw::MAX_PARTS;
use crate::midi_map::KeptMap;
use crate::name::{ProjectName, SoundName};
use crate::orbit::scene::OrbitSong;
use crate::params::EngineType;
use crate::preset::Sound;
pub use crate::storage::ProjectFile;
use crate::storage::ProjectId;

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
    #[optimize(size)]
    fn new_project() -> Self {
        ProjectMeta {
            file: None,
            name: NEW_PROJECT_NAME,
            saved_crc: None,
        }
    }

    #[optimize(size)]
    pub fn id(&self) -> Option<ProjectId> {
        self.file.map(ProjectFile::id)
    }

    #[optimize(size)]
    pub fn file(&self) -> Option<ProjectFile> {
        self.file
    }

    #[optimize(size)]
    pub fn name(&self) -> ProjectName {
        self.name
    }

    #[optimize(size)]
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
    /// The slot's bytes didn't decode: never, as they were checked when
    /// stored; the Part is left as it was.
    Unreadable,
    /// A library load the pool has no room for; `None`: no slot free.
    Full(Option<PoolFull>),
}

/// The Parts a project names a controller map for: none without
/// `controller-maps`, so a bench build's save writes no pick.
pub(crate) const CTL_PARTS: usize = if cfg!(feature = "controller-maps") {
    MAX_PARTS
} else {
    0
};

pub struct Project {
    meta: ProjectMeta,
    pool: Pool,
    perf: Performance,
    /// ORBIT's scene and tempo: project data, as the Parts are.
    orbit: OrbitSong,
    /// Each Part's map record a closed door left unread, for the save
    /// (#815): here, not on the Sound, so no Sound copy carries it.
    kept: [KeptMap; MAX_PARTS],
    /// Each Part's controller map, by name (#807, ADR 0226): the map
    /// itself is a card file, read again on every load. None without
    /// `controller-maps` (the bench builds, no MIDI MAP): their D2 is full.
    ctl: [Option<SoundName>; CTL_PARTS],
    /// Each Part's kit (ADR 0265): an ITCM static lent at build, as the
    /// pool's arena is, since D2 is full.
    kits: &'static mut Kits,
    /// Moves on every `&mut` path, so a status keyed on it is never stale.
    rev: u32,
}

crate::in_place::field_list!(Project => Project { meta, pool, perf, orbit, kept, ctl, kits, rev });

impl Project {
    /// Wrapping; any two reads with an edit between differ.
    #[optimize(size)]
    pub fn rev(&self) -> u32 {
        self.rev
    }

    /// Every `&mut` path calls this first: nothing reads between it and the edit.
    #[optimize(size)]
    fn bump(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    #[optimize(size)]
    pub fn meta(&self) -> &ProjectMeta {
        &self.meta
    }

    #[optimize(size)]
    pub fn perf(&self) -> &Performance {
        &self.perf
    }

    #[optimize(size)]
    pub fn pool(&self) -> &Pool {
        &self.pool
    }

    /// Slot `s` damaged (`Pool::cut_for_test`).
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[optimize(size)]
    pub fn cut_slot_for_test(&mut self, s: SlotId) {
        self.bump();
        self.pool.cut_for_test(s);
    }

    /// `Pool::sweep_scratch`: the boot's card sweep, before the project
    /// loads.
    #[optimize(size)]
    pub fn sweep_scratch(&mut self) -> &mut [u8] {
        self.pool.sweep_scratch()
    }

    #[optimize(size)]
    pub fn part(&self, p: PartId) -> &Part {
        self.perf.part(p)
    }

    /// A Part read as the pages read it; can't be written through.
    #[optimize(size)]
    pub fn read_part(&self, p: PartId) -> PartRead<'_> {
        let x = self.part(p);
        PartRead {
            sound: &x.sound,
            mix: &x.mix,
            fx: &self.perf.fx,
        }
    }

    #[optimize(size)]
    pub fn edit_part(&mut self, p: PartId) -> PartEdit<'_> {
        self.bump();
        self.perf.edit(p)
    }

    /// Part `p`'s kit (ADR 0265).
    #[optimize(size)]
    pub fn kit(&self, p: PartId) -> &crate::dsp::drum::types::Kit {
        self.kits.kit(p)
    }

    /// Every Part's kit, as the audio's view is published from.
    #[optimize(size)]
    pub fn kits(&self) -> &Kits {
        self.kits
    }

    /// Part `p`'s selected pad: the UI's, not the project's, so it moves
    /// no revision.
    #[optimize(size)]
    pub fn selected_pad(&self, p: PartId) -> crate::dsp::drum::types::PadId {
        self.kits.selected(p)
    }

    /// Inlined into each caller: bank 2's `ed` calls it too (ADR 0285),
    /// and bank 1 keeps no out-of-line copy for it.
    #[inline(always)]
    pub fn select_pad(&mut self, p: PartId, pad: crate::dsp::drum::types::PadId) {
        self.kits.select(p, pad);
    }

    /// Part `p`'s pads lit by live hits: the UI's, so no revision.
    #[optimize(size)]
    pub fn pad_lights(&self, p: PartId) -> PadLights {
        self.kits.lights(p)
    }

    /// A live hit lit Part `p`'s `pad`.
    #[optimize(size)]
    pub fn light_pad(&mut self, p: PartId, pad: crate::dsp::drum::types::PadId) {
        self.kits.light(p, pad);
    }

    /// Part `p`'s pads its voices sound, from the audio: lit while they do.
    #[optimize(size)]
    pub fn sound_pads(&mut self, p: PartId, pads: crate::dsp::drum::types::PadSet) {
        self.kits.sounding(p, pads);
    }

    /// One UI frame, `frame` long, on for the lit pads and PAD FOLLOW's
    /// holds.
    #[optimize(size)]
    pub fn fade_pad_lights(&mut self, frame: chimera_hal::units::Millis) {
        self.kits.fade(frame);
    }

    /// Part `p`'s PAD FOLLOW IDLE hold since its last pad edit.
    #[optimize(size)]
    pub fn pad_follow_hold(&self, p: PartId) -> FollowHold {
        self.kits.hold(p)
    }

    /// A pad edit on Part `p`'s DRUM pages: IDLE's hits there leave the pad
    /// a while.
    #[optimize(size)]
    pub fn pad_edited(&mut self, p: PartId) {
        self.kits.edited(p);
    }

    #[optimize(size)]
    pub fn kit_mut(&mut self, p: PartId) -> &mut crate::dsp::drum::types::Kit {
        self.bump();
        self.kits.kit_mut(p)
    }

    #[optimize(size)]
    pub fn edit_fx(&mut self) -> &mut FxParams {
        self.bump();
        &mut self.perf.fx
    }

    #[optimize(size)]
    pub fn orbit(&self) -> &OrbitSong {
        &self.orbit
    }

    #[optimize(size)]
    pub fn orbit_mut(&mut self) -> &mut OrbitSong {
        self.bump();
        &mut self.orbit
    }

    /// Part `p`'s controller map's name, if it has one (ADR 0226).
    #[optimize(size)]
    pub fn controller(&self, p: PartId) -> Option<SoundName> {
        self.ctl.get(p.index()).copied().flatten()
    }

    /// Part `p` plays controller map `m`, or none: a project edit.
    #[optimize(size)]
    pub fn set_controller(&mut self, p: PartId, m: Option<SoundName>) {
        self.bump();
        if let Some(c) = self.ctl.get_mut(p.index()) {
            *c = m;
        }
    }

    /// Part `p`'s expression (CC 11, ADR 0263): played, never saved.
    #[optimize(size)]
    pub fn set_expression(&mut self, p: PartId, v: u8) {
        self.bump();
        self.perf.parts[p.index()].expression = v;
    }

    /// Part `p`'s CC 73 (`which` 0) or 72 (1) offset applied (ADR 0263).
    #[optimize(size)]
    pub fn set_env_offset(&mut self, p: PartId, which: usize, v: i8) {
        self.bump();
        self.perf.parts[p.index()].env[which & 1] = v;
    }

    /// Part `p`'s TUNING plays Scala file `s` (ADR 0259): a project edit.
    /// Nothing without `tuning`.
    #[optimize(size)]
    pub fn set_scala(&mut self, p: PartId, s: Option<crate::tuning::ScalaName>) {
        self.bump();
        #[cfg(feature = "tuning")]
        {
            self.perf.parts[p.index()].scala = s.unwrap_or_default();
        }
        #[cfg(not(feature = "tuning"))]
        let _ = (p, s);
    }

    /// KEEP MAP (ADR 0253): Part `p`'s Sound as it is now reads Clean
    /// (its kept rows written over a load's), or, `false`, as its slot
    /// says. Bank 2's (`bank2.x`): only the MIDI UI calls it.
    #[cfg(feature = "controller-maps")]
    #[inline(never)]
    #[optimize(size)]
    pub fn set_map_kept(&mut self, p: PartId, kept: bool) {
        self.bump();
        let part = &mut self.perf.parts[p.index()];
        part.map_kept = kept.then(|| crate::storage::sound_crc(&part.sound));
    }

    /// Whether Part `p` reads Clean by KEEP MAP's rows (ADR 0253).
    #[cfg(feature = "controller-maps")]
    #[optimize(size)]
    pub fn map_kept(&self, p: PartId) -> bool {
        self.perf.parts[p.index()].map_kept.is_some()
    }

    #[optimize(size)]
    pub fn set_name(&mut self, n: ProjectName) {
        self.bump();
        self.meta.name = n;
    }

    /// The Parts whose Origin is slot `s`.
    #[optimize(size)]
    pub fn users(&self, s: SlotId) -> PartSet {
        PartId::ALL
            .into_iter()
            .filter(|&p| matches!(self.part(p).origin, Origin::Slot { slot, .. } if slot == s))
            .fold(PartSet::EMPTY, PartSet::with)
    }

    #[optimize(size)]
    pub fn pool_store(&mut self, s: SlotId, sound: &Sound) -> Result<(), PoolFull> {
        self.bump();
        self.pool.store(s, sound, &KeptMap::EMPTY)
    }

    #[optimize(size)]
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
    /// use chimera_engine::project::{PartId, Project, SlotId};
    /// fn f(p: &mut Project) {
    ///     let _ = p.save_part_to(PartId::ALL[0], SlotId::ALL[0]);
    /// }
    /// ```
    #[optimize(size)]
    pub(crate) fn save_part_to(&mut self, p: PartId, s: SlotId) -> Result<PartSet, PoolFull> {
        self.bump();
        let others = self.users(s);
        self.pool
            .store(s, &self.perf.parts[p.index()].sound, &self.kept[p.index()])?;
        let stored = self.pool.get(s).expect("just stored");
        // Saved as it sounds: CC 64 is this Sound now (ADR 0263).
        self.perf.parts[p.index()].env = [0; 2];
        self.perf.parts[p.index()].origin = Origin::Slot {
            slot: s,
            generation: self.pool.generation(s),
            crc: stored.crc(),
        };
        Ok(others
            .iter()
            .filter(|&o| o != p && sound_status(self.part(o), &self.pool) == SoundStatus::Stale(s))
            .fold(PartSet::EMPTY, PartSet::with))
    }

    /// Replaces the Part's Sound; its mix stays. Refused if the Part moved
    /// since it was confirmed, or the slot emptied.
    #[optimize(size)]
    pub fn replace_part(&mut self, c: Confirmed<PartSource>) -> Result<(), ReplaceError> {
        if !c.holds(self) {
            return Err(ReplaceError::Changed);
        }
        self.load_part(c.target())
    }

    /// A program change's load (ADR 0224), the one that needs no guard: it
    /// loses nothing, as the Part's Sound, edited or not, is swapped into
    /// `keep` first. Then `adopt`: what came out of `keep` stays, as
    /// `src`'s Sound edited (a recall copy back); else `src` loads over it,
    /// and a refused load swaps the Part's Sound back.
    /// Bank 2's (`bank2.x`): only the MIDI UI calls it.
    #[inline(never)]
    #[optimize(size)]
    pub fn program_load(
        &mut self,
        src: PartSource,
        keep: &mut Sound,
        adopt: bool,
    ) -> Result<(), ReplaceError> {
        let origin = match src.from {
            PartFrom::Slot(s) => {
                let sound = self.pool.get(s).ok_or(ReplaceError::SlotEmpty)?;
                Origin::Slot {
                    slot: s,
                    generation: self.pool.generation(s),
                    crc: sound.crc(),
                }
            }
            PartFrom::Init(e) => Origin::Init(e),
        };
        self.bump();
        let i = src.part.index();
        core::mem::swap(&mut *self.perf.parts[i].sound, keep);
        if !adopt {
            // Refused, the Part is as it was: its Sound back from `keep`,
            // its loads unmoved.
            let (loads, env) = (self.perf.parts[i].loads, self.perf.parts[i].env);
            let r = self.load_part(src);
            if r.is_err() {
                let part = &mut self.perf.parts[i];
                core::mem::swap(&mut *part.sound, keep);
                (part.loads, part.env) = (loads, env);
            }
            return r;
        }
        let part = &mut self.perf.parts[i];
        part.loads.bump();
        part.env = [0; 2];
        part.origin = origin;
        Ok(())
    }

    /// Only through `replace_part`, `program_load` or a Part action.
    #[optimize(size)]
    fn load_part(&mut self, src: PartSource) -> Result<(), ReplaceError> {
        self.bump();
        let part = &mut self.perf.parts[src.part.index()];
        part.loads.bump();
        part.env = [0; 2];
        // Cleared only once the load can't fail: a refused one keeps it.
        let kept = &mut self.kept[src.part.index()];
        match src.from {
            PartFrom::Slot(s) => {
                let sound = self.pool.get(s).ok_or(ReplaceError::SlotEmpty)?;
                sound
                    .decode_into(&mut part.sound)
                    .map_err(|_| ReplaceError::Unreadable)?;
                kept.clear();
                sound.keep_map(kept);
                part.origin = Origin::Slot {
                    slot: s,
                    generation: self.pool.generation(s),
                    crc: sound.crc(),
                };
            }
            PartFrom::Init(e) => {
                *part.sound = Sound::init(e);
                kept.clear();
                part.origin = Origin::Init(e);
            }
        }
        Ok(())
    }
}
