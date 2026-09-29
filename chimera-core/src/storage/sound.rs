//! A Sound as card records: `Engine` first, a `Block` per voice block, then
//! `Registry`, `ModDests` and `Routes` (ADR 0045).

use chimera_hal::store::StoreError;

use crate::addr::{BlockRef, Blocks, ParamAddr};
use crate::block::{DiskCode, ParamId};
use crate::mod_path::{LABEL_LEN, MAX_REGISTRY_DESTS, ModDestRegistry};
use crate::modulation::{MAX_MOD_DESTS, MAX_MOD_SOURCES, ModSource, ModState};
use crate::name::SoundName;
use crate::params::{EngineType, ParamSnapshot};
use crate::preset::Sound;

use super::block_codec::{ByteSet, decode_block, encode_block};
use super::codes::{MIGRATIONS, ValidAddr};
use super::frame::{Event, FileError, FileKind};
use super::record::{MAX_RECORD_LEN, ReadTag, RecordBuf, RecordTag, RecordWriter};

const REGISTRY_ENTRY_LEN: usize = 2 + LABEL_LEN;
const ROUTE_LEN: usize = 4;
const MAX_ROUTES: usize = MAX_MOD_SOURCES * MAX_MOD_DESTS;
const _: () = assert!(MAX_ROUTES * ROUTE_LEN <= MAX_RECORD_LEN);
const _: () = assert!(MAX_REGISTRY_DESTS * REGISTRY_ENTRY_LEN <= MAX_RECORD_LEN);

impl Sound {
    /// The frozen base a file decodes onto: every param at its default, no
    /// routes, an empty registry. Not `init`, whose routes may change.
    pub fn neutral(engine: EngineType) -> Sound {
        Sound {
            name: Sound::init_name(),
            params: ParamSnapshot::for_engine(engine),
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

/// The stored address with these codes, if this firmware has it.
fn addr_of(block: u8, id: u8) -> Option<ParamAddr> {
    let b = BlockRef::from_disk_code(block)?;
    ValidAddr::find(b, ParamId(id)).map(ValidAddr::addr)
}

pub fn encode_sound(s: &Sound, w: &mut RecordWriter<'_>) -> Result<(), StoreError> {
    w.put(RecordTag::Engine, &[s.engine().disk_code()])?;
    for b in BlockRef::ALL {
        if let Some(blk) = s.params.block(b) {
            let mut r = RecordBuf::new();
            encode_block(b, blk, &mut r);
            w.put(RecordTag::Block, r.as_slice())?;
        }
    }

    let mut r = RecordBuf::new();
    for e in (0..s.dest_registry.len()).filter_map(|i| s.dest_registry.get(i)) {
        r.bytes(&addr_code(e.addr));
        r.bytes(&e.label);
    }
    w.put(RecordTag::Registry, r.as_slice())?;

    let m = &s.mod_state;
    let mut r = RecordBuf::new();
    r.u8(m.num_sources() as u8);
    for d in 0..m.num_dests() {
        r.bytes(&addr_code(m.dest(d)));
    }
    w.put(RecordTag::ModDests, r.as_slice())?;

    let mut r = RecordBuf::new();
    for d in 0..m.num_dests() {
        for src in ModSource::ALL.into_iter().take(m.num_sources()) {
            if m.present(d) & 1 << src.index() != 0 {
                r.u8(src.disk_code());
                r.bytes(&addr_code(m.dest(d)));
                r.u8(m.amount(src.index(), d) as u8);
            }
        }
    }
    w.put(RecordTag::Routes, r.as_slice())
}

/// A record's bit in `SoundDecoder::seen`; `Block` repeats per block code.
const fn once_bit(tag: RecordTag) -> u8 {
    match tag {
        RecordTag::Block => 0,
        RecordTag::Engine => 1,
        RecordTag::Registry => 1 << 1,
        RecordTag::ModDests => 1 << 2,
        RecordTag::Routes => 1 << 3,
        RecordTag::LastProject => 1 << 4,
    }
}

/// Decodes a Sound file's events onto `target`, one pass at a time: with
/// `apply` false it only checks, and a pass starts at the header.
pub struct SoundDecoder<'a> {
    target: &'a mut Sound,
    name: Option<SoundName>,
    /// The Engine record was read: every other record may follow.
    engine: bool,
    /// The records read, so none we write once repeats: a bit per singleton
    /// tag, and the block codes.
    seen: u8,
    blocks: ByteSet,
    /// Applied in `end`, once the `ModDests` they index are known.
    routes: [u8; MAX_RECORD_LEN],
    routes_len: usize,
}

impl<'a> SoundDecoder<'a> {
    pub fn new(target: &'a mut Sound) -> Self {
        SoundDecoder {
            target,
            name: None,
            engine: false,
            seen: 0,
            blocks: ByteSet::new(),
            routes: [0; MAX_RECORD_LEN],
            routes_len: 0,
        }
    }
}

impl SoundDecoder<'_> {
    /// `NeedsNewerFirmware`: an unknown engine. `Corrupt`: a record before
    /// `Engine`, or one we write once read twice. `Bounds`: a payload of the
    /// wrong shape, or more entries than the Sound holds. `WrongKind`: not a
    /// Sound file.
    pub fn event(&mut self, e: Event<'_>, apply: bool) -> Result<(), FileError> {
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
                return Ok(());
            }
            Event::Record(ReadTag::Known(tag), p) => (tag, p),
            Event::Record(ReadTag::Unknown(_), _) if self.engine => return Ok(()),
            Event::Record(ReadTag::Unknown(_), _) => return Err(FileError::Corrupt),
        };
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
            RecordTag::Engine => self.engine_record(p, apply),
            RecordTag::Block => {
                let target: Option<&mut dyn Blocks> = if apply {
                    Some(&mut self.target.params)
                } else {
                    None
                };
                decode_block(p, MIGRATIONS, target)
            }
            RecordTag::Registry => self.registry(p, apply),
            RecordTag::ModDests => self.mod_dests(p, apply),
            RecordTag::Routes => {
                if !p.len().is_multiple_of(ROUTE_LEN) || p.len() / ROUTE_LEN > MAX_ROUTES {
                    return Err(FileError::Bounds);
                }
                self.routes[..p.len()].copy_from_slice(p);
                self.routes_len = p.len();
                Ok(())
            }
            RecordTag::LastProject => Ok(()),
        }
    }

    /// `Corrupt` without an Engine record. Applies the routes.
    pub fn end(&mut self, apply: bool) -> Result<(), FileError> {
        if !self.engine {
            return Err(FileError::Corrupt);
        }
        if apply {
            let m = &mut self.target.mod_state;
            let (routes, _) = self.routes[..self.routes_len].as_chunks::<ROUTE_LEN>();
            for &[src, block, id, amount] in routes {
                let (Some(src), Some(addr)) = (ModSource::from_disk_code(src), addr_of(block, id))
                else {
                    continue;
                };
                if let Some(d) = m.find(addr).or_else(|| m.push(addr)) {
                    m.set_route(src.index(), d, amount as i8);
                }
            }
        }
        Ok(())
    }

    fn engine_record(&mut self, p: &[u8], apply: bool) -> Result<(), FileError> {
        let &[code] = p else {
            return Err(FileError::Bounds);
        };
        let engine = EngineType::from_disk_code(code).ok_or(FileError::NeedsNewerFirmware)?;
        self.engine = true;
        if apply {
            *self.target = Sound::neutral(engine);
            if let Some(n) = self.name {
                self.target.name = n;
            }
        }
        Ok(())
    }

    fn registry(&mut self, p: &[u8], apply: bool) -> Result<(), FileError> {
        let (entries, []) = p.as_chunks::<REGISTRY_ENTRY_LEN>() else {
            return Err(FileError::Bounds);
        };
        if entries.len() > MAX_REGISTRY_DESTS {
            return Err(FileError::Bounds);
        }
        if apply {
            for &[block, id, ref label @ ..] in entries {
                let Some(addr) = addr_of(block, id) else {
                    continue;
                };
                // Refused when not modulatable: skipped.
                let _ = self.target.dest_registry.add(addr, *label);
            }
        }
        Ok(())
    }

    fn mod_dests(&mut self, p: &[u8], apply: bool) -> Result<(), FileError> {
        let (&sources, dests) = p.split_first().ok_or(FileError::Bounds)?;
        let (dests, []) = dests.as_chunks::<2>() else {
            return Err(FileError::Bounds);
        };
        if dests.len() > MAX_MOD_DESTS {
            return Err(FileError::Bounds);
        }
        if apply {
            let mut m = ModState::with_sources(sources.into());
            for &[block, id] in dests {
                // `push` refuses one that isn't modulatable: skipped.
                if let Some(addr) = addr_of(block, id) {
                    m.push(addr);
                }
            }
            self.target.mod_state = m;
        }
        Ok(())
    }
}
