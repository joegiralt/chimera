//! A Sound as card records: `Engine` first, a `Block` per voice block, then
//! `Registry`, `ModDests` and `Routes` (ADR 0045).

use chimera_hal::store::StoreError;

use crate::addr::{BlockRead, BlockRef, Blocks, ParamAddr};
use crate::block::{DiskCode, ParamId};
use crate::mod_path::{LABEL_LEN, MAX_REGISTRY_DESTS, ModDestRegistry};
use crate::modulation::{FixedCol, MAX_MOD_DESTS, MAX_MOD_SOURCES, ModSource, ModState};
use crate::name::SoundName;
use crate::params::{EngineType, FilterParams, ParamSnapshot};
use crate::preset::Sound;
use crate::taps::Jack;

use super::block_codec::{ByteSet, ENTRY_LEN, decode_block, encode_block};
use super::codes::{DiskAddr, MIGRATIONS, TRANSLATIONS, ValidAddr, readdress};
use super::file::{Check, Decode};
use super::frame::{Event, FileError, FileKind};
use super::record::records_crc;
use super::record::{MAX_RECORD_LEN, ReadTag, RecordBuf, RecordTag, RecordWriter};

const REGISTRY_ENTRY_LEN: usize = 2 + LABEL_LEN;
const ROUTE_LEN: usize = 4;
/// Every source into every registry column and every fixed one.
const MAX_ROUTES: usize = MAX_MOD_SOURCES * (MAX_MOD_DESTS + FixedCol::ALL.len());
const _: () = assert!(MAX_ROUTES * ROUTE_LEN <= MAX_RECORD_LEN);
const _: () = assert!(MAX_REGISTRY_DESTS * REGISTRY_ENTRY_LEN <= MAX_RECORD_LEN);

impl Sound {
    /// The frozen base a file decodes onto: every param at its v1 default,
    /// no routes, an empty registry. Not `init`, whose routes may change,
    /// nor today's defaults: OUT LEVEL 0.8 and RR 8, as before ADR 0063
    /// moved INIT's.
    pub fn neutral(engine: EngineType) -> Sound {
        let mut params = ParamSnapshot::for_engine(engine);
        params.out.volume = 0.8;
        for o in &mut params.algo.ops {
            o.rr = 8;
        }
        Sound {
            name: Sound::init_name(),
            params,
            mod_state: ModState::with_sources(MAX_MOD_SOURCES),
            dest_registry: ModDestRegistry::new(),
        }
    }
}

/// `a`'s (block code, param id). Every address a Sound routes is modulatable,
/// so its block is stored.
fn addr_code(a: ParamAddr) -> [u8; 2] {
    [a.block.disk_code().expect("a stored block"), a.param.0]
}

/// The stored address with these codes, if this firmware has it; a
/// retired one through `READDRESS` (DRIVE's routes land on LEVEL).
fn addr_of(block: u8, id: u8) -> Option<ParamAddr> {
    if let Some(a) = readdress(DiskAddr {
        block,
        id: ParamId(id),
    }) {
        return Some(a);
    }
    let b = BlockRef::from_disk_code(block)?;
    ValidAddr::find(b, ParamId(id)).map(ValidAddr::addr)
}

pub fn encode_sound(s: &Sound, w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    w.put(RecordTag::Engine, &[s.engine().disk_code()])?;
    // One buffer for every record: this frame is charged to each pointer
    // call (ADR 0025's stack check), so it holds one, not one per record.
    let mut r = RecordBuf::new();
    for b in BlockRef::ALL {
        if let Some(blk) = s.params.block(b) {
            r.clear();
            encode_block(b, blk, &mut r);
            w.put(RecordTag::Block, r.as_slice())?;
        }
    }

    r.clear();
    for e in (0..s.dest_registry.len()).filter_map(|i| s.dest_registry.get(i)) {
        r.bytes(&addr_code(e.addr));
        r.bytes(&e.label);
    }
    w.put(RecordTag::Registry, r.as_slice())?;

    let m = &s.mod_state;
    r.clear();
    r.u8(m.num_sources() as u8);
    for d in 0..m.num_dests() {
        r.bytes(&addr_code(m.dest(d)));
    }
    w.put(RecordTag::ModDests, r.as_slice())?;

    r.clear();
    for d in 0..m.num_dests() {
        for src in ModSource::ALL.into_iter().take(m.num_sources()) {
            if m.present(d) & 1 << src.index() != 0 {
                r.u8(src.disk_code());
                r.bytes(&addr_code(m.dest(d)));
                r.u8(m.amount(src.index(), d) as u8);
            }
        }
    }
    // The fixed columns: ordinary route records, by address (no ModDests).
    for c in FixedCol::ALL {
        for src in ModSource::ALL.into_iter().take(m.num_sources()) {
            if m.fixed_present(c) & 1 << src.index() != 0 {
                r.u8(src.disk_code());
                r.bytes(&addr_code(c.addr()));
                r.u8(m.fixed_amount(src.index(), c) as u8);
            }
        }
    }
    w.put(RecordTag::Routes, r.as_slice())
}

/// The CRC of the Sound's padded name, then `encode_sound`'s bytes: equal
/// for Sounds a card round trip can't tell apart.
pub fn sound_crc(s: &Sound) -> u32 {
    records_crc(&s.name.padded(), |w| encode_sound(s, w))
}

/// A record's bit in `SoundCheck::seen`; `Block` repeats per block code.
const fn once_bit(tag: RecordTag) -> u8 {
    match tag {
        RecordTag::Block => 0,
        RecordTag::Engine => 1,
        RecordTag::Registry => 1 << 1,
        RecordTag::ModDests => 1 << 2,
        RecordTag::Routes => 1 << 3,
        RecordTag::LastProject => 1 << 4,
        // A project's own records: refused before `once` is asked.
        RecordTag::Slot
        | RecordTag::Part
        | RecordTag::Fx
        | RecordTag::Origin
        | RecordTag::Tempo
        | RecordTag::OrbitBlock
        | RecordTag::OrbitNotes
        | RecordTag::OrbitChord
        | RecordTag::Orbit
        | RecordTag::OrbitScale => 0,
    }
}

/// Checks a Sound file's events: pass 1, which touches nothing. A pass
/// starts at the header. `SoundDecoder` runs the same checks in pass 2,
/// onto the Sound it stages.
pub struct SoundCheck {
    name: Option<SoundName>,
    /// The Engine record was read: every other record may follow.
    engine: bool,
    /// The records read, so none we write once repeats: a bit per singleton
    /// tag, and the block codes.
    seen: u8,
    blocks: ByteSet,
    /// Applied at the end, once the `ModDests` they index are known.
    routes: [u8; MAX_RECORD_LEN],
    routes_len: usize,
    /// The Filter block record held FM3 IN and QM IN: else `infer_jacks`.
    had_fm3: bool,
    had_qm: bool,
}

impl SoundCheck {
    pub fn new() -> Self {
        Self::begin(None)
    }

    /// A Sound past its header: a project's context, whose record names it.
    pub(crate) fn begin(name: Option<SoundName>) -> Self {
        SoundCheck {
            name,
            engine: false,
            seen: 0,
            blocks: ByteSet::new(),
            routes: [0; MAX_RECORD_LEN],
            routes_len: 0,
            had_fm3: false,
            had_qm: false,
        }
    }
}

impl Default for SoundCheck {
    fn default() -> Self {
        Self::new()
    }
}

impl Check for SoundCheck {
    const KIND: FileKind = FileKind::Sound;

    fn event(&mut self, e: Event<'_>) -> Result<(), FileError> {
        self.step(e, None)
    }

    fn end(&mut self) -> Result<(), FileError> {
        self.finish(None)
    }
}

/// Decodes a Sound file onto `target`: pass 1 checks, pass 2 builds the
/// Sound in `staged`, and `commit` swaps it into `target`.
pub struct SoundDecoder<'a> {
    check: SoundCheck,
    target: &'a mut Sound,
    staged: Sound,
}

impl<'a> SoundDecoder<'a> {
    pub fn new(target: &'a mut Sound) -> Self {
        SoundDecoder {
            check: SoundCheck::new(),
            target,
            staged: Sound::neutral(EngineType::Algo),
        }
    }
}

impl Check for SoundDecoder<'_> {
    const KIND: FileKind = FileKind::Sound;

    fn event(&mut self, e: Event<'_>) -> Result<(), FileError> {
        self.check.event(e)
    }

    fn end(&mut self) -> Result<(), FileError> {
        self.check.end()
    }
}

impl Decode for SoundDecoder<'_> {
    fn apply(&mut self, e: Event<'_>) -> Result<(), FileError> {
        self.check.step(e, Some(&mut self.staged))
    }

    fn commit(&mut self) -> Result<(), FileError> {
        self.check.finish(Some(&mut self.staged))?;
        core::mem::swap(self.target, &mut self.staged);
        Ok(())
    }
}

impl SoundCheck {
    /// One event, applied to `staged` if there is one.
    ///
    /// `NeedsNewerFirmware`: an unknown engine. `Corrupt`: a record before
    /// `Engine`, one we write once read twice, or a project's record.
    /// `Bounds`: a payload of the wrong shape, or more entries than the
    /// Sound holds. `WrongKind`: not a Sound file.
    pub(crate) fn step(
        &mut self,
        e: Event<'_>,
        staged: Option<&mut Sound>,
    ) -> Result<(), FileError> {
        let (tag, p) = match e {
            Event::Header(h) => {
                if h.kind != FileKind::Sound {
                    return Err(FileError::WrongKind);
                }
                self.name = h.name;
                self.engine = false;
                self.seen = 0;
                self.blocks = ByteSet::new();
                self.routes_len = 0;
                (self.had_fm3, self.had_qm) = (false, false);
                return Ok(());
            }
            Event::Record(ReadTag::Known(tag), p) => (tag, p),
            Event::Record(ReadTag::Unknown(_), _) if self.engine => return Ok(()),
            Event::Record(ReadTag::Unknown(_), _) => return Err(FileError::Corrupt),
        };
        if matches!(
            tag,
            RecordTag::Slot | RecordTag::Part | RecordTag::Fx | RecordTag::Origin
        ) || tag.is_orbit()
        {
            return Err(FileError::Corrupt);
        }
        // Engine first, and once.
        if (tag == RecordTag::Engine) == self.engine {
            return Err(FileError::Corrupt);
        }
        let once = match tag {
            RecordTag::Block => p.first().is_none_or(|&b| self.blocks.insert(b)),
            _ => {
                let bit = once_bit(tag);
                let fresh = self.seen & bit == 0;
                self.seen |= bit;
                fresh
            }
        };
        if !once {
            return Err(FileError::Corrupt);
        }
        match tag {
            RecordTag::Engine => self.engine_record(p, staged),
            RecordTag::Block => {
                if p.first() == BlockRef::Filter.disk_code().as_ref() {
                    let has = |id: ParamId| p[1..].chunks(ENTRY_LEN).any(|e| e[0] == id.0);
                    self.had_fm3 = has(FilterParams::FM3_IN);
                    self.had_qm = has(FilterParams::QM_IN);
                }
                decode_block(
                    p,
                    MIGRATIONS,
                    TRANSLATIONS,
                    staged.map(|s| &mut s.params as &mut dyn Blocks),
                )
            }
            RecordTag::Registry => registry(p, staged),
            RecordTag::ModDests => mod_dests(p, staged),
            RecordTag::Routes => {
                if !p.len().is_multiple_of(ROUTE_LEN) || p.len() / ROUTE_LEN > MAX_ROUTES {
                    return Err(FileError::Bounds);
                }
                self.routes[..p.len()].copy_from_slice(p);
                self.routes_len = p.len();
                Ok(())
            }
            RecordTag::LastProject => Ok(()),
            RecordTag::Slot
            | RecordTag::Part
            | RecordTag::Fx
            | RecordTag::Origin
            | RecordTag::Tempo
            | RecordTag::OrbitBlock
            | RecordTag::OrbitNotes
            | RecordTag::OrbitChord
            | RecordTag::Orbit
            | RecordTag::OrbitScale => Err(FileError::Corrupt),
        }
    }

    /// `Corrupt` without an Engine record. Adds the routes to `staged`.
    pub(crate) fn finish(&mut self, staged: Option<&mut Sound>) -> Result<(), FileError> {
        if !self.engine {
            return Err(FileError::Corrupt);
        }
        if let Some(s) = staged {
            let m = &mut s.mod_state;
            let (routes, _) = self.routes[..self.routes_len].as_chunks::<ROUTE_LEN>();
            for &[src, block, id, amount] in routes {
                let (Some(src), Some(addr)) = (ModSource::from_disk_code(src), addr_of(block, id))
                else {
                    continue;
                };
                // A fixed column's route lands there; any other finds or
                // takes its registry column, else is dropped.
                m.set_route_addr(src.index(), addr, amount as i8);
            }
            infer_jacks(s, self.had_fm3, self.had_qm);
        }
        Ok(())
    }

    fn engine_record(&mut self, p: &[u8], staged: Option<&mut Sound>) -> Result<(), FileError> {
        let &[code] = p else {
            return Err(FileError::Bounds);
        };
        let engine = EngineType::from_disk_code(code).ok_or(FileError::NeedsNewerFirmware)?;
        self.engine = true;
        if let Some(s) = staged {
            *s = Sound::neutral(engine);
            if let Some(n) = self.name {
                s.name = n;
            }
        }
        Ok(())
    }
}

/// A patch saved before the jacks (filter-types spec § 2): FM3 IN is the
/// single fixed-FREQ route that is neither FM1's ENV 1 nor FM2's NOTE, QM
/// IN the single fixed-Q route; otherwise each keeps its default.
fn infer_jacks(s: &mut Sound, had_fm3: bool, had_qm: bool) {
    let single = |bits: u8| (bits.count_ones() == 1).then(|| bits.trailing_zeros() as usize);
    let m = &s.mod_state;
    let fm1_fm2 = 1 << ModSource::Env1.index() | 1 << ModSource::Note.index();
    let fm3 = single(m.fixed_present(FixedCol::Freq) & !fm1_fm2);
    let qm = single(m.fixed_present(FixedCol::Q));
    let f = &mut s.params.filter;
    if !had_fm3 && let Some(i) = fm3 {
        f.set_fm3_src(Jack::Mod(ModSource::ALL[i]));
    }
    if !had_qm && let Some(i) = qm {
        f.set_qm_src(Jack::Mod(ModSource::ALL[i]));
    }
}

fn registry(p: &[u8], staged: Option<&mut Sound>) -> Result<(), FileError> {
    let (entries, []) = p.as_chunks::<REGISTRY_ENTRY_LEN>() else {
        return Err(FileError::Bounds);
    };
    if entries.len() > MAX_REGISTRY_DESTS {
        return Err(FileError::Bounds);
    }
    if let Some(s) = staged {
        for &[block, id, ref label @ ..] in entries {
            let Some(addr) = addr_of(block, id) else {
                continue;
            };
            // Refused when not modulatable or a fixed column's: skipped.
            let _ = s.dest_registry.add(addr, *label);
        }
    }
    Ok(())
}

fn mod_dests(p: &[u8], staged: Option<&mut Sound>) -> Result<(), FileError> {
    let (&sources, dests) = p.split_first().ok_or(FileError::Bounds)?;
    let (dests, []) = dests.as_chunks::<2>() else {
        return Err(FileError::Bounds);
    };
    if dests.len() > MAX_MOD_DESTS {
        return Err(FileError::Bounds);
    }
    if let Some(s) = staged {
        let mut m = ModState::with_sources(sources.into());
        for &[block, id] in dests {
            // `push` refuses one that isn't modulatable, or is a fixed
            // column's (its routes load there): skipped. One
            // already there (DRIVE's beside LEVEL's) is not added twice.
            if let Some(addr) = addr_of(block, id)
                && m.find(addr).is_none()
            {
                m.push(addr);
            }
        }
        s.mod_state = m;
    }
    Ok(())
}
