//! A project as card records (projects spec § Format): a `PoolSource` per
//! pool source (ADR 0087: its ref, never audio), `Fx` and its five
//! blocks, then `Slot` and its Sound per filled slot, then each `Part`: its
//! Sound, `Block(Part)` (the mix) and `Origin`; then ORBIT's records
//! (`orbit::records`). `Slot`, `Part` and `Fx` open a context; the records
//! after one belong to it. An ORBIT record closes the last Part's.
//!
//! A slot's Sound is decoded whole, then encoded into the pool's arena as
//! `encode_sound` writes it today (ADR 0087): a file from before sparse
//! Sounds loads sparse.
//!
//! Pass 1 costs the slots and the sources' refs, with no card read. Before
//! pass 2, one listing of `/CHIMERA/TAPES` tells which refs are on the
//! card, and the picked side's plan takes, against the whole arena: the
//! sources a Part plays, then the slots by index, then the other sources,
//! each while it fits. Then the load's epoch fades every voice, and pass 2
//! starts only once no voice holds a source (ADR 0099). Pass 2
//! retires the old sources into a drained arena and follows the plan; each
//! source becomes an absent entry, and the loader reads the planned ones
//! from the card after it.

use chimera_hal::store::{Dir, Store, StoreError};

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
    Check, DecodeInPlace, Event, FileError, FileKind, LoadError, MIGRATIONS, ReadTag, Ready,
    RecordBuf, RecordTag, RecordWriter, SoundCheck, TRANSLATIONS, decode_block, encode_block,
    encode_sound, records_crc, source_file, source_file_min_len,
};

use super::arena::{ARENA_BYTES, Cost, MAX_SOURCES};
use super::pool::{Pool, entry_len};
use super::sources::{SourceId, SourceRef, ref_cost};
use super::swap::{LoadLink, Swap};
use super::template::{new_project_part, new_slot};
use super::{EncodedSound, Origin, Part, PartId, PartSet, Project, SlotId};
use crate::preset::MAX_SOUNDS;
use crate::storage::Side;

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

/// A pool slot: its bytes are the records `put_slot` would write.
fn put_encoded(
    w: &mut RecordWriter<'_>,
    s: SlotId,
    sound: EncodedSound<'_>,
) -> Result<(), StoreError> {
    put_context(w, RecordTag::Slot, s.index(), sound.name())?;
    w.copy(sound.records())
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

/// The pool's source refs, absent ones included, ordered by ref: one file
/// for one set of sources, whatever their slots.
fn put_sources(w: &mut RecordWriter<'_>, pool: &Pool) -> Result<(), StoreError> {
    let mut refs = SourceId::ALL.map(|s| pool.source(s).map(|e| e.r));
    refs.sort_unstable_by_key(|r| r.map(|r| (r.crc, r.frames, r.rate)));
    for r in refs.into_iter().flatten() {
        w.put(RecordTag::PoolSource, &r.to_bytes())?;
    }
    Ok(())
}

/// Streams `p` from live state: no copy, no buffer past one record.
pub fn encode_project(p: &Project, w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    put_sources(w, &p.pool)?;
    put_fx(w, &p.perf.fx)?;
    for s in SlotId::ALL {
        if let Some(sound) = p.pool.get(s) {
            put_encoded(w, s, sound)?;
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

/// What a load takes of a file (ADR 0087): its slots and sources (a bit
/// each, sources by `PoolSource` record in file order), the sources not on
/// the card, and how many it leaves out for want of room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Plan {
    slots: u128,
    sources: u32,
    missing: u32,
    dropped_slots: u8,
    dropped_sources: u8,
}

impl Plan {
    /// No plan made: take everything the arena does.
    const ALL: Plan = Plan {
        slots: u128::MAX,
        sources: u32::MAX,
        missing: 0,
        dropped_slots: 0,
        dropped_sources: 0,
    };
}

/// One side's pass 1, what its plan needs: the slots read and each one's
/// arena bytes, the `PoolSource` refs in file order, and the ones a Part's
/// Sound plays (a bit each in `refs`).
#[derive(Clone, Copy, Debug)]
struct Costs {
    slots: u128,
    slot_costs: [u16; MAX_SOUNDS],
    refs: [SourceRef; MAX_SOURCES],
    n_refs: u8,
    part_used: u32,
}

impl Costs {
    const EMPTY: Costs = Costs {
        slots: 0,
        slot_costs: [0; MAX_SOUNDS],
        refs: [SourceRef {
            crc: 0,
            frames: 0,
            rate: 0,
        }; MAX_SOURCES],
        n_refs: 0,
        part_used: 0,
    };

    fn refs(&self) -> &[SourceRef] {
        &self.refs[..self.n_refs as usize]
    }
}

/// Takes `c` from `left` if it fits.
fn take(left: &mut u32, c: Option<Cost>) -> bool {
    match c {
        Some(c) if c.bytes() <= *left => {
            *left -= c.bytes();
            true
        }
        _ => false,
    }
}

/// Checks a project file's events: pass 1, which touches nothing.
/// `ProjectDecoder` runs the same checks in pass 2, writing its target.
pub struct ProjectCheck {
    /// The open context's Sound: a fresh one per `Slot` and `Part`.
    sound: SoundCheck,
    /// The pass 1 of `side`, the side read now (`ProjectDecoder` keeps the
    /// other's).
    costs: Costs,
    side: Side,
    /// Pass 2's plan, made before it (`ProjectDecoder::begin`), and what
    /// it left out so far.
    plan: Plan,
    dropped_slots: u8,
    dropped_sources: u8,
    /// Pass 2's absent entries the plan takes: the loader reads them.
    to_read: u32,
    context: Context,
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

const _: () = assert!(SlotId::ALL.len() <= u128::BITS as usize);

impl Default for ProjectCheck {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectCheck {
    pub fn new() -> Self {
        ProjectCheck {
            sound: SoundCheck::begin(None),
            costs: Costs::EMPTY,
            side: Side::A,
            plan: Plan::ALL,
            dropped_slots: 0,
            dropped_sources: 0,
            to_read: 0,
            context: Context::None,
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
    /// one, a `PoolSource` after `Fx` or repeated. `Bounds`: an index out
    /// of range, a payload of the wrong shape, or a 33rd `PoolSource`.
    /// `BadName`: a context's name. An ORBIT record is never an error past
    /// its place: a bad one keeps the neutral base.
    ///
    /// `stage`: where pass 1 of a load decodes each `Slot`'s Sound whole,
    /// to cost it. Pass 2 decodes into `target`'s pool scratch and stores
    /// it. A check alone holds no Sound.
    fn step(
        &mut self,
        e: Event<'_>,
        target: Option<&mut Project>,
        stage: Option<&mut Sound>,
    ) -> Result<(), FileError> {
        let (tag, p) = match e {
            Event::Header(h) => {
                if h.kind != FileKind::Project {
                    return Err(FileError::WrongKind);
                }
                let name = h.name.ok_or(FileError::Corrupt)?;
                self.restart();
                if let Some(t) = target {
                    (self.dropped_slots, self.dropped_sources) =
                        (self.plan.dropped_slots, self.plan.dropped_sources);
                    // Every slot cleared: generations only move. The old
                    // sources retire, and their room must be free before
                    // anything is stored: pass 1 planned on the whole
                    // arena. Only an audio that stopped answering since
                    // `begin` leaves one pending.
                    t.pool.clear_all();
                    if !t.pool.sources_drain_timed() {
                        return Err(FileError::Bounds);
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
            (ReadTag::Known(t), _) if t.is_orbit() => self.orbit(t, p, target, stage),
            (ReadTag::Unknown(_), Context::Orbit) => Ok(()),
            (ReadTag::Known(_), Context::Orbit) => Err(FileError::Corrupt),
            (ReadTag::Known(RecordTag::PoolSource), Context::None) => self.pool_source(p, target),
            (ReadTag::Known(RecordTag::PoolSource), _) => Err(FileError::Corrupt),
            (ReadTag::Known(t @ (RecordTag::Fx | RecordTag::Slot | RecordTag::Part)), _) => {
                self.open(t, p, target, stage)
            }
            (ReadTag::Unknown(_), Context::None | Context::Fx) => Ok(()),
            (ReadTag::Known(_), Context::None) => Err(FileError::Corrupt),
            (ReadTag::Known(RecordTag::Block), Context::Fx) => self.fx_block(p, target),
            (ReadTag::Known(_), Context::Fx) => Err(FileError::Corrupt),
            (_, Context::Slot(_)) => {
                let staged = stage.or(target.map(|t| t.pool.scratch()));
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
                if let Event::Record(ReadTag::Known(RecordTag::SourceRef), p) = e
                    && let Some(r) = SourceRef::from_bytes(p)
                    && let Some(k) = self.costs().refs().iter().position(|&x| x == r)
                {
                    self.costs_mut().part_used |= 1 << k;
                }
                let staged = target.map(|t| &mut t.perf.parts[n.index()].sound);
                self.sound.step(e, staged)
            }
        }
    }

    /// A pass's fresh start, in place: no whole `ProjectCheck` built on
    /// the stack. The side and the plan stay.
    fn restart(&mut self) {
        self.sound = SoundCheck::begin(None);
        let c = self.costs_mut();
        (c.slots, c.n_refs, c.part_used) = (0, 0, 0);
        c.slot_costs.fill(0);
        (self.dropped_slots, self.dropped_sources, self.to_read) = (0, 0, 0);
        (self.context, self.parts) = (Context::None, PartSet::EMPTY);
        (self.fx, self.fx_blocks, self.mix, self.origin) = (false, 0, false, false);
        self.origins = [ReadOrigin::Missing; MAX_PARTS];
        self.lossy = false;
    }

    fn costs(&self) -> &Costs {
        &self.costs
    }

    fn costs_mut(&mut self) -> &mut Costs {
        &mut self.costs
    }

    /// A `PoolSource`: its ref, each once. Pass 2 makes it an absent
    /// entry, and marks it for the card read if the plan takes it.
    fn pool_source(&mut self, p: &[u8], target: Option<&mut Project>) -> Result<(), FileError> {
        let r = SourceRef::from_bytes(p).ok_or(FileError::Bounds)?;
        let c = self.costs_mut();
        let k = c.refs().len();
        if k == MAX_SOURCES {
            return Err(FileError::Bounds);
        }
        if c.refs().contains(&r) {
            return Err(FileError::Corrupt);
        }
        (c.refs[k], c.n_refs) = (r, c.n_refs + 1);
        let Some(t) = target else { return Ok(()) };
        let bit = 1 << k;
        let planned = self.plan.sources & bit != 0;
        match t.pool.source_absent(r) {
            Some(id) if planned => self.to_read |= id.bit(),
            Some(_) => {}
            // No slot free: the ref is lost, and a save would drop it. A
            // ref the plan left out for room is counted already.
            None => {
                if planned || self.plan.missing & bit != 0 {
                    self.dropped_sources += 1;
                }
                self.lossy = true;
            }
        }
        Ok(())
    }

    /// An ORBIT record: the first closes the last Part, so all six must be
    /// read; then into the song.
    fn orbit(
        &mut self,
        tag: RecordTag,
        p: &[u8],
        mut target: Option<&mut Project>,
        stage: Option<&mut Sound>,
    ) -> Result<(), FileError> {
        if self.context != Context::Orbit {
            if PartId::ALL.iter().any(|&n| !self.parts.contains(n)) {
                return Err(FileError::Corrupt);
            }
            self.close(target.as_deref_mut(), stage)?;
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
        stage: Option<&mut Sound>,
    ) -> Result<(), FileError> {
        self.close(target.as_deref_mut(), stage)?;
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
            let c = self.costs_mut();
            if c.slots & bit != 0 {
                return Err(FileError::Corrupt);
            }
            c.slots |= bit;
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
    fn close(
        &mut self,
        target: Option<&mut Project>,
        stage: Option<&mut Sound>,
    ) -> Result<(), FileError> {
        match self.context {
            Context::Slot(s) => match (target, stage) {
                (Some(t), _) => {
                    self.sound.finish(Some(t.pool.scratch()))?;
                    // A source still sounding from the last project can
                    // hold what the plan counted on: that slot didn't fit.
                    let planned = self.plan.slots & 1 << s.index() != 0;
                    if planned && t.pool.store_scratch(s).is_err() {
                        self.dropped_slots += 1;
                    }
                    Ok(())
                }
                (None, Some(stage)) => {
                    self.sound.finish(Some(&mut *stage))?;
                    let c = Cost::of(entry_len(stage)).bytes();
                    self.costs_mut().slot_costs[s.index()] = c as u16;
                    Ok(())
                }
                (None, None) => self.sound.finish(None),
            },
            Context::Part(n) => {
                let staged = target.map(|t| &mut t.perf.parts[n.index()].sound);
                self.sound.finish(staged)
            }
            Context::None | Context::Fx | Context::Orbit => Ok(()),
        }
    }

    /// The plan of this side's pass 1, against the whole arena: the
    /// sources a Part plays, the slots by index, then the other sources,
    /// each taken while it fits. A source not `present` on the card is
    /// neither taken nor counted.
    fn plan_load(&self, present: u32) -> Plan {
        let c = self.costs();
        let mut left = ARENA_BYTES as u32;
        let refs = ((1u64 << c.n_refs) - 1) as u32;
        let mut p = Plan {
            slots: 0,
            sources: 0,
            missing: refs & !present,
            dropped_slots: 0,
            dropped_sources: 0,
        };
        let sources = |part: bool, p: &mut Plan, left: &mut u32| {
            for (k, &r) in c.refs().iter().enumerate() {
                if (c.part_used & 1 << k != 0) != part || p.missing & 1 << k != 0 {
                    continue;
                }
                if take(left, ref_cost(r)) {
                    p.sources |= 1 << k;
                } else {
                    p.dropped_sources += 1;
                }
            }
        };
        sources(true, &mut p, &mut left);
        for s in SlotId::ALL
            .into_iter()
            .filter(|s| c.slots & 1 << s.index() != 0)
        {
            let cost = Cost::of(c.slot_costs[s.index()] as usize);
            if take(&mut left, Some(cost)) {
                p.slots |= 1 << s.index();
            } else {
                p.dropped_slots += 1;
            }
        }
        sources(false, &mut p, &mut left);
        p
    }

    /// Closes the last context; `Corrupt` unless all six Parts were read.
    /// Then resolves each Part's Origin against the pool as loaded.
    fn finish(
        &mut self,
        mut target: Option<&mut Project>,
        stage: Option<&mut Sound>,
    ) -> Result<(), FileError> {
        self.close(target.as_deref_mut(), stage)?;
        // A file from before ORBIT: a save adds its records.
        self.lossy |= self.context != Context::Orbit;
        self.context = Context::None;
        if PartId::ALL.iter().any(|&n| !self.parts.contains(n)) {
            return Err(FileError::Corrupt);
        }
        let Some(t) = target else {
            return Ok(());
        };
        // A save would write fewer slots than the file holds.
        self.lossy |= self.dropped_slots > 0;
        t.orbit.loaded();
        for (part, read) in t.perf.parts.iter_mut().zip(self.origins) {
            let engine = part.sound.engine();
            part.origin = match read {
                ReadOrigin::Slot(slot) => match t.pool.get(slot) {
                    Some(s) => Origin::Slot {
                        slot,
                        generation: t.pool.generation(slot),
                        crc: s.crc(),
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
        self.step(e, None, None)
    }

    fn end(&mut self) -> Result<(), FileError> {
        self.finish(None, None)
    }

    fn side(&mut self, s: Side) {
        self.side = s;
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
    /// The pass 1 costs of the side `check` isn't on: the plan is made for
    /// the side picked, after both are read.
    other: Costs,
    target: &'a mut Project,
    /// The epoch `begin` bumps; none at boot, where nothing sounds.
    link: Option<&'a LoadLink>,
    swap: Option<Swap>,
}

impl<'a> ProjectDecoder<'a> {
    pub(crate) fn new(target: &'a mut Project, link: Option<&'a LoadLink>) -> Self {
        target.bump();
        ProjectDecoder {
            check: ProjectCheck::new(),
            other: Costs::EMPTY,
            target,
            link,
            swap: None,
        }
    }

    /// `new` with no epoch, for tests.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn new_for_test(target: &'a mut Project) -> Self {
        Self::new(target, None)
    }

    /// The epoch `begin` bumped, if it did: the caller settles it and
    /// publishes, whatever came of the load.
    pub(crate) fn take_swap(&mut self) -> Option<Swap> {
        self.swap.take()
    }

    /// After pass 2: the sources the loader reads from the card, each an
    /// absent entry until it does.
    pub(crate) fn to_read(&self) -> u32 {
        self.check.to_read
    }

    /// After `begin`: the refs `/CHIMERA/TAPES` doesn't list.
    pub(crate) fn missing(&self) -> u8 {
        self.check.plan.missing.count_ones() as u8
    }

    /// After pass 2: the Sounds and sources the arena didn't take.
    pub(crate) fn dropped(&self) -> (u8, u8) {
        (self.check.dropped_slots, self.check.dropped_sources)
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
        // The pool's scratch only: nothing a pass 1 refusal could leave.
        self.check.step(e, None, Some(self.target.pool.scratch()))
    }

    fn end(&mut self) -> Result<(), FileError> {
        self.check.finish(None, Some(self.target.pool.scratch()))
    }

    /// Keeps each side's costs: the side read last moves to `other`.
    fn side(&mut self, s: Side) {
        if s != self.check.side {
            core::mem::swap(&mut self.check.costs, &mut self.other);
        }
        self.check.side = s;
    }
}

impl DecodeInPlace for ProjectDecoder<'_> {
    /// Plans the picked side against the refs `/CHIMERA/TAPES` lists (names
    /// and sizes, no data read), then bumps the epoch (ADR 0046's kill) and
    /// waits for every voice to let go of its source. `Busy` when one
    /// doesn't: nothing is touched, and the caller still publishes.
    fn begin<S: Store>(&mut self, s: &mut S, r: &Ready) -> Result<(), LoadError> {
        let mut present = 0u32;
        let refs = self.check.costs().refs();
        let listed = s.list(r.volume(), Dir::Tapes, &mut |name, size| {
            for (k, &x) in refs.iter().enumerate() {
                if name == source_file(x) && source_file_min_len(x).is_some_and(|n| size >= n) {
                    present |= 1 << k;
                }
            }
        });
        match listed {
            Ok(()) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(LoadError::Store(e)),
        }
        self.check.plan = self.check.plan_load(present);
        self.swap = self.link.map(LoadLink::bump);
        let acked = |s: &Swap| self.link.is_some_and(|l| l.acked(s.epoch()));
        if !self
            .target
            .pool
            .quiet_timed(|| self.swap.as_ref().is_none_or(acked))
        {
            return Err(LoadError::Busy);
        }
        Ok(())
    }

    fn apply(&mut self, e: Event<'_>) -> Result<(), FileError> {
        self.check.step(e, Some(self.target), None)
    }

    fn finish(&mut self) -> Result<(), FileError> {
        self.check.finish(Some(self.target), None)
    }
}
