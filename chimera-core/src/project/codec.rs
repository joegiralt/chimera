//! A project as card records (projects spec § Format): `Fx` and its five
//! blocks, then `Slot` and its Sound per filled slot, then each `Part`: its
//! Sound, `Block(Part)` (the mix) and `Origin`; then ORBIT's records
//! (`orbit::records`). `Slot`, `Part` and `Fx` open a context; the records
//! after one belong to it. An ORBIT record closes the last Part's.

use chimera_hal::store::StoreError;

use crate::addr::{BlockRead, BlockRef, Blocks};
use crate::block::{Block, DiskCode};
use crate::dsp::fx_bus::FxParams;
use crate::hw::MAX_PARTS;
use crate::name::SoundName;
use crate::orbit::records::{encode_new_orbit, encode_orbit, read as read_orbit};
use crate::params::EngineType;
use crate::part::PartParams;
use crate::preset::Sound;
use crate::storage::{
    Check, DecodeInPlace, Event, FileError, FileKind, MIGRATIONS, ReadTag, RecordBuf, RecordTag,
    RecordWriter, SoundCheck, TRANSLATIONS, decode_block, encode_block, encode_sound, records_crc,
    sound_crc,
};

use super::template::{new_project_part, new_slot};
use super::{Origin, Part, PartId, PartSet, Project, SlotId};

const FX_BLOCKS: [BlockRef; 5] = [
    BlockRef::Chorus,
    BlockRef::Delay,
    BlockRef::Reverb,
    BlockRef::Tape,
    BlockRef::Comp,
];

const PART_CODE: u8 = match BlockRef::Part.disk_code() {
    Some(c) => c,
    None => panic!("PART is stored"),
};

/// A `Slot` or `Part` payload: the index, then the padded name.
const CONTEXT_LEN: usize = 1 + 16;

const ORIGIN_SLOT: u8 = 0;
const ORIGIN_INIT: u8 = 1;

fn fx_block(fx: &FxParams, b: BlockRef) -> Option<&dyn Block> {
    Some(match b {
        BlockRef::Chorus => &fx.chorus,
        BlockRef::Delay => &fx.delay,
        BlockRef::Reverb => &fx.reverb,
        BlockRef::Tape => &fx.tape,
        BlockRef::Comp => &fx.comp,
        _ => return None,
    })
}

/// The FX as the blocks a `Block` record can reach in the `Fx` context.
struct FxBlocks<'a>(&'a mut FxParams);

impl BlockRead for FxBlocks<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        fx_block(self.0, b)
    }
}

impl Blocks for FxBlocks<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        let fx = &mut *self.0;
        Some(match b {
            BlockRef::Chorus => &mut fx.chorus,
            BlockRef::Delay => &mut fx.delay,
            BlockRef::Reverb => &mut fx.reverb,
            BlockRef::Tape => &mut fx.tape,
            BlockRef::Comp => &mut fx.comp,
            _ => return None,
        })
    }
}

/// A Part's mix as the one block `Block(Part)` reaches.
struct MixBlocks<'a>(&'a mut PartParams);

impl BlockRead for MixBlocks<'_> {
    fn block(&self, b: BlockRef) -> Option<&dyn Block> {
        (b == BlockRef::Part).then_some(&*self.0 as &dyn Block)
    }
}

impl Blocks for MixBlocks<'_> {
    fn block_mut(&mut self, b: BlockRef) -> Option<&mut dyn Block> {
        (b == BlockRef::Part).then_some(&mut *self.0 as &mut dyn Block)
    }
}

fn put_block(w: &mut RecordWriter<'_>, b: BlockRef, blk: &dyn Block) -> Result<(), StoreError> {
    let mut r = RecordBuf::new();
    encode_block(b, blk, None, &mut r);
    w.put(RecordTag::Block, r.as_slice())
}

fn put_context(
    w: &mut RecordWriter<'_>,
    tag: RecordTag,
    index: usize,
    name: SoundName,
) -> Result<(), StoreError> {
    let mut p = [0; CONTEXT_LEN];
    p[0] = index as u8;
    p[1..].copy_from_slice(&name.padded());
    w.put(tag, &p)
}

fn put_fx(w: &mut RecordWriter<'_>, fx: &FxParams) -> Result<(), StoreError> {
    w.put(RecordTag::Fx, &[])?;
    for b in FX_BLOCKS {
        if let Some(blk) = fx_block(fx, b) {
            put_block(w, b, blk)?;
        }
    }
    Ok(())
}

fn put_slot(w: &mut RecordWriter<'_>, s: SlotId, sound: &Sound) -> Result<(), StoreError> {
    put_context(w, RecordTag::Slot, s.index(), sound.name)?;
    encode_sound(sound, w)
}

fn put_part(w: &mut RecordWriter<'_>, i: usize, part: &Part) -> Result<(), StoreError> {
    put_context(w, RecordTag::Part, i, part.sound.name)?;
    encode_sound(&part.sound, w)?;
    put_block(w, BlockRef::Part, &part.mix)?;
    let origin = match part.origin {
        Origin::Slot { slot, .. } => [ORIGIN_SLOT, slot.index() as u8],
        Origin::Init(e) => [ORIGIN_INIT, e.disk_code()],
    };
    w.put(RecordTag::Origin, &origin)
}

/// Streams `p` from live state: no copy, no buffer past one record.
pub fn encode_project(p: &Project, w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    put_fx(w, &p.perf.fx)?;
    for s in SlotId::ALL {
        if let Some(sound) = p.pool.get(s) {
            put_slot(w, s, sound)?;
        }
    }
    for (i, part) in p.perf.parts.iter().enumerate() {
        put_part(w, i, part)?;
    }
    encode_orbit(&p.orbit, w)
}

/// NEW's records, as `encode_project` of NEW puts them, built a Sound at a
/// time: no second project in RAM (CLEAR of a file not loaded).
pub fn encode_new_project(w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    put_fx(w, &FxParams::default())?;
    for s in SlotId::ALL {
        if let Some(sound) = new_slot(s) {
            put_slot(w, s, &sound)?;
        }
    }
    for i in 0..MAX_PARTS {
        put_part(w, i, &new_project_part(i))?;
    }
    encode_new_orbit(w)
}

/// The project's canonical CRC: its padded name, then `encode_project`'s
/// bytes. Content only: generations and the header don't count.
pub fn project_crc(p: &Project) -> u32 {
    records_crc(&p.meta.name.padded(), |w| encode_project(p, w))
}

/// The context the records belong to: the last `Fx`, `Slot` or `Part`,
/// or ORBIT's, which is last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Context {
    None,
    Fx,
    Slot(SlotId),
    Part(PartId),
    Orbit,
}

/// A Part's `Origin` record as read; `finish` resolves it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReadOrigin {
    Missing,
    Slot(SlotId),
    /// `None`: an engine (or an origin kind) this firmware doesn't know.
    Init(Option<EngineType>),
}

/// Checks a project file's events: pass 1, which touches nothing.
/// `ProjectDecoder` runs the same checks in pass 2, writing its target.
pub struct ProjectCheck {
    /// The open context's Sound: a fresh one per `Slot` and `Part`.
    sound: SoundCheck,
    context: Context,
    slots: u32,
    parts: PartSet,
    fx: bool,
    /// The FX blocks read, a bit each in `FX_BLOCKS` order.
    fx_blocks: u8,
    /// The open Part's mix and Origin were read.
    mix: bool,
    origin: bool,
    origins: [ReadOrigin; MAX_PARTS],
    /// Pass 2 took less than the file holds: an ORBIT record skipped or a
    /// value in it defaulted, or no ORBIT records at all (ADR 0078). A save
    /// would write other bytes, so the load is not Saved.
    lossy: bool,
}

const _: () = assert!(SlotId::ALL.len() <= u32::BITS as usize);

impl Default for ProjectCheck {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectCheck {
    pub fn new() -> Self {
        ProjectCheck {
            sound: SoundCheck::begin(None),
            context: Context::None,
            slots: 0,
            parts: PartSet::EMPTY,
            fx: false,
            fx_blocks: 0,
            mix: false,
            origin: false,
            origins: [ReadOrigin::Missing; MAX_PARTS],
            lossy: false,
        }
    }

    /// One event, written into `target` if there is one.
    ///
    /// `WrongKind`: not a project. `Corrupt`: a nameless header, a known
    /// record outside any context, a context repeated, a record the context
    /// doesn't hold or holds once read twice, a Sound's rule broken, an
    /// ORBIT record before the six Parts, or anything but ORBIT's after
    /// one. `Bounds`: an index out of range or a payload of the wrong
    /// shape. `BadName`: a context's name. An ORBIT record is never an
    /// error past its place: a bad one keeps the neutral base.
    fn step(&mut self, e: Event<'_>, target: Option<&mut Project>) -> Result<(), FileError> {
        let (tag, p) = match e {
            Event::Header(h) => {
                if h.kind != FileKind::Project {
                    return Err(FileError::WrongKind);
                }
                let name = h.name.ok_or(FileError::Corrupt)?;
                *self = ProjectCheck::new();
                if let Some(t) = target {
                    // Every slot through `clear`: generations only move.
                    for s in SlotId::ALL {
                        t.pool.clear(s);
                    }
                    t.perf.fx = FxParams::default();
                    t.meta.name = name;
                    t.orbit.clear_for_load();
                }
                return Ok(());
            }
            Event::Record(tag, p) => (tag, p),
        };
        match (tag, self.context) {
            (ReadTag::Known(t), _) if t.is_orbit() => self.orbit(t, p, target),
            (ReadTag::Unknown(_), Context::Orbit) => Ok(()),
            (ReadTag::Known(_), Context::Orbit) => Err(FileError::Corrupt),
            (ReadTag::Known(t @ (RecordTag::Fx | RecordTag::Slot | RecordTag::Part)), _) => {
                self.open(t, p, target)
            }
            (ReadTag::Unknown(_), Context::None | Context::Fx) => Ok(()),
            (ReadTag::Known(_), Context::None) => Err(FileError::Corrupt),
            (ReadTag::Known(RecordTag::Block), Context::Fx) => self.fx_block(p, target),
            (ReadTag::Known(_), Context::Fx) => Err(FileError::Corrupt),
            (_, Context::Slot(s)) => {
                let staged = target.and_then(|t| t.pool.slots[s.index()].as_mut());
                self.sound.step(e, staged)
            }
            (ReadTag::Known(RecordTag::Block), Context::Part(n))
                if p.first() == Some(&PART_CODE) =>
            {
                if core::mem::replace(&mut self.mix, true) {
                    return Err(FileError::Corrupt);
                }
                let mut mix = target.map(|t| MixBlocks(&mut t.perf.parts[n.index()].mix));
                decode_block(
                    p,
                    MIGRATIONS,
                    TRANSLATIONS,
                    mix.as_mut().map(|m| m as &mut dyn Blocks),
                )
            }
            (ReadTag::Known(RecordTag::Origin), Context::Part(n)) => {
                if core::mem::replace(&mut self.origin, true) {
                    return Err(FileError::Corrupt);
                }
                self.origins[n.index()] = match *p {
                    [ORIGIN_SLOT, s] => ReadOrigin::Slot(SlotId::new(s).ok_or(FileError::Bounds)?),
                    [ORIGIN_INIT, code] => ReadOrigin::Init(EngineType::from_disk_code(code)),
                    [_, _] => ReadOrigin::Init(None),
                    _ => return Err(FileError::Bounds),
                };
                Ok(())
            }
            (_, Context::Part(n)) => {
                let staged = target.map(|t| &mut t.perf.parts[n.index()].sound);
                self.sound.step(e, staged)
            }
        }
    }

    /// An ORBIT record: the first closes the last Part, so all six must be
    /// read; then into the song.
    fn orbit(
        &mut self,
        tag: RecordTag,
        p: &[u8],
        mut target: Option<&mut Project>,
    ) -> Result<(), FileError> {
        if self.context != Context::Orbit {
            if PartId::ALL.iter().any(|&n| !self.parts.contains(n)) {
                return Err(FileError::Corrupt);
            }
            self.close(target.as_deref_mut())?;
            self.context = Context::Orbit;
        }
        if !read_orbit(tag, p, target.map(|t| &mut t.orbit)) {
            self.lossy = true;
        }
        Ok(())
    }

    /// Closes the open context, then opens `tag`'s.
    fn open(
        &mut self,
        tag: RecordTag,
        p: &[u8],
        mut target: Option<&mut Project>,
    ) -> Result<(), FileError> {
        self.close(target.as_deref_mut())?;
        if tag == RecordTag::Fx {
            if !p.is_empty() {
                return Err(FileError::Bounds);
            }
            if core::mem::replace(&mut self.fx, true) {
                return Err(FileError::Corrupt);
            }
            self.context = Context::Fx;
            return Ok(());
        }
        let Ok(&[index, ref name @ ..]) = <&[u8; CONTEXT_LEN]>::try_from(p) else {
            return Err(FileError::Bounds);
        };
        let name = SoundName::from_padded(name).map_err(|_| FileError::BadName)?;
        if tag == RecordTag::Slot {
            let s = SlotId::new(index).ok_or(FileError::Bounds)?;
            let bit = 1 << s.index();
            if self.slots & bit != 0 {
                return Err(FileError::Corrupt);
            }
            self.slots |= bit;
            if let Some(t) = target {
                t.pool.store(s, Sound::neutral(EngineType::Algo));
            }
            self.context = Context::Slot(s);
        } else {
            let n = PartId::new(index).ok_or(FileError::Bounds)?;
            if self.parts.contains(n) {
                return Err(FileError::Corrupt);
            }
            self.parts = self.parts.with(n);
            if let Some(t) = target {
                let part = &mut t.perf.parts[n.index()];
                part.sound = Sound::neutral(EngineType::Algo);
                part.mix = PartParams::for_part(n.index());
                part.origin = Origin::Init(EngineType::Algo);
            }
            (self.mix, self.origin) = (false, false);
            self.context = Context::Part(n);
        }
        self.sound = SoundCheck::begin(Some(name));
        Ok(())
    }

    /// A `Block` in `Fx`: an FX block, each once. A block this firmware
    /// doesn't know is checked and skipped.
    fn fx_block(&mut self, p: &[u8], target: Option<&mut Project>) -> Result<(), FileError> {
        let &code = p.first().ok_or(FileError::Bounds)?;
        match FX_BLOCKS.iter().position(|b| b.disk_code() == Some(code)) {
            Some(i) => {
                let bit = 1 << i;
                if self.fx_blocks & bit != 0 {
                    return Err(FileError::Corrupt);
                }
                self.fx_blocks |= bit;
            }
            None if BlockRef::from_disk_code(code).is_some() => return Err(FileError::Corrupt),
            None => {}
        }
        let mut fx = target.map(|t| FxBlocks(&mut t.perf.fx));
        decode_block(
            p,
            MIGRATIONS,
            TRANSLATIONS,
            fx.as_mut().map(|f| f as &mut dyn Blocks),
        )
    }

    /// Ends a `Slot` or `Part`: its Sound's `finish`, which adds the routes.
    fn close(&mut self, target: Option<&mut Project>) -> Result<(), FileError> {
        match self.context {
            Context::Slot(s) => {
                let staged = target.and_then(|t| t.pool.slots[s.index()].as_mut());
                self.sound.finish(staged)
            }
            Context::Part(n) => {
                let staged = target.map(|t| &mut t.perf.parts[n.index()].sound);
                self.sound.finish(staged)
            }
            Context::None | Context::Fx | Context::Orbit => Ok(()),
        }
    }

    /// Closes the last context; `Corrupt` unless all six Parts were read.
    /// Then resolves each Part's Origin against the pool as loaded.
    fn finish(&mut self, mut target: Option<&mut Project>) -> Result<(), FileError> {
        self.close(target.as_deref_mut())?;
        // A file from before ORBIT: a save adds its records.
        self.lossy |= self.context != Context::Orbit;
        self.context = Context::None;
        if PartId::ALL.iter().any(|&n| !self.parts.contains(n)) {
            return Err(FileError::Corrupt);
        }
        let Some(t) = target else { return Ok(()) };
        t.orbit.loaded();
        for (part, read) in t.perf.parts.iter_mut().zip(self.origins) {
            let engine = part.sound.engine();
            part.origin = match read {
                ReadOrigin::Slot(slot) => match t.pool.get(slot) {
                    Some(s) => Origin::Slot {
                        slot,
                        generation: t.pool.generation(slot),
                        crc: sound_crc(s),
                    },
                    None => Origin::Init(engine),
                },
                ReadOrigin::Init(Some(e)) => Origin::Init(e),
                ReadOrigin::Init(None) | ReadOrigin::Missing => Origin::Init(engine),
            };
        }
        Ok(())
    }
}

impl Check for ProjectCheck {
    const KIND: FileKind = FileKind::Project;

    fn event(&mut self, e: Event<'_>) -> Result<(), FileError> {
        self.step(e, None)
    }

    fn end(&mut self) -> Result<(), FileError> {
        self.finish(None)
    }
}

/// Decodes a project file into `target` in place: pass 1 checks, pass 2
/// writes as it reads. There is no staging copy, so a pass 2 that fails
/// leaves `target` part written (the loader falls back to NEW).
///
/// It replaces the whole project, so only the guarded load makes one:
///
/// ```compile_fail,E0624
/// use chimera_core::project::{Project, ProjectDecoder};
/// fn f(p: &mut Project) {
///     let _ = ProjectDecoder::new(p);
/// }
/// ```
pub struct ProjectDecoder<'a> {
    check: ProjectCheck,
    target: &'a mut Project,
}

impl<'a> ProjectDecoder<'a> {
    pub(crate) fn new(target: &'a mut Project) -> Self {
        target.bump();
        ProjectDecoder {
            check: ProjectCheck::new(),
            target,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn new_for_test(target: &'a mut Project) -> Self {
        Self::new(target)
    }

    /// After pass 2: the project holds less than the file, or other bytes
    /// than a save would write (ADR 0078). The load is then not Saved.
    pub fn lossy(&self) -> bool {
        self.check.lossy
    }
}

impl Check for ProjectDecoder<'_> {
    const KIND: FileKind = FileKind::Project;

    fn event(&mut self, e: Event<'_>) -> Result<(), FileError> {
        self.check.event(e)
    }

    fn end(&mut self) -> Result<(), FileError> {
        self.check.end()
    }
}

impl DecodeInPlace for ProjectDecoder<'_> {
    fn apply(&mut self, e: Event<'_>) -> Result<(), FileError> {
        self.check.step(e, Some(self.target))
    }

    fn finish(&mut self) -> Result<(), FileError> {
        self.check.finish(Some(self.target))
    }
}
