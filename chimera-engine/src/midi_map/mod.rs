//! A Sound's MIDI map (spec §§ 2-3): up to 16 CC -> param entries, a fixed
//! array with no heap. `Mappable` is the whitelist type, so an address a CC
//! may not drive (FX, ORBIT, settings) can never be stored.

pub use crate::midi_in::Cc;

use crate::addr::{BlockRef, ParamAddr};
use crate::part::PartParams;

/// A `ParamAddr` a CC may drive: a Sound's own, stored, non-patching
/// params, and the Part's LEVEL and PAN (CC 7 and 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mappable(ParamAddr);

impl Mappable {
    pub const fn new(a: ParamAddr) -> Option<Self> {
        let ok = if matches!(a.block, BlockRef::Part) {
            a.param.0 == PartParams::LEVEL.0 || a.param.0 == PartParams::PAN.0
        } else if a.block.in_sound() {
            match a.spec() {
                Some(s) => !s.patching && s.stored,
                None => false,
            }
        } else {
            false
        };
        if ok { Some(Self(a)) } else { None }
    }

    pub const fn addr(self) -> ParamAddr {
        self.0
    }
}

/// One mapping. `min` and `max` are places 0..=255 of the param's
/// normalized span; `min > max` inverts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapEntry {
    pub cc: Cc,
    pub target: Mappable,
    pub min: u8,
    pub max: u8,
}

impl MapEntry {
    /// A CC value 0..=127 as a place 0..1 of the span.
    pub fn place(&self, v: u8) -> f32 {
        let (lo, hi) = (self.min as f32, self.max as f32);
        (lo + (hi - lo) * (v.min(127) as f32 / 127.0)) / 255.0
    }
}

pub const MAP_LEN: usize = 16;

const CC0: Cc = match Cc::new(0) {
    Some(c) => c,
    None => panic!(),
};

/// Sixteen mappings are held and one more was learned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapFull;

#[derive(Clone, Copy)]
pub struct MidiMap {
    entries: [MapEntry; MAP_LEN],
    len: u8,
}

impl core::fmt::Debug for MidiMap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.entries()).finish()
    }
}

/// Equal by the live entries: slots past `len` are scratch.
impl PartialEq for MidiMap {
    fn eq(&self, o: &Self) -> bool {
        self.entries() == o.entries()
    }
}

impl Eq for MidiMap {}

impl MidiMap {
    pub const EMPTY: Self = Self {
        entries: [MapEntry {
            cc: CC0,
            target: Mappable(ParamAddr::new(BlockRef::Filter, crate::block::ParamId(0))),
            min: 0,
            max: 255,
        }; MAP_LEN],
        len: 0,
    };

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn entries(&self) -> &[MapEntry] {
        &self.entries[..self.len()]
    }

    /// Maps `cc` to `t` over the full range: replaces the target's entry
    /// (a param appears once) or appends.
    pub fn learn(&mut self, cc: Cc, t: Mappable) -> Result<(), MapFull> {
        let e = MapEntry {
            cc,
            target: t,
            min: 0,
            max: 255,
        };
        let n = self.len();
        if let Some(i) = self.entries().iter().position(|x| x.target == t) {
            self.entries[i] = e;
        } else if n < MAP_LEN {
            self.entries[n] = e;
            self.len += 1;
        } else {
            return Err(MapFull);
        }
        Ok(())
    }

    /// Deletes entry `i`, keeping learn order. Out of range does nothing.
    pub fn remove(&mut self, i: usize) {
        let n = self.len();
        if i < n {
            self.entries.copy_within(i + 1..n, i);
            self.len -= 1;
        }
    }

    /// Replaces entry `i`. Out of range does nothing.
    pub fn set(&mut self, i: usize, e: MapEntry) {
        if i < self.len() {
            self.entries[i] = e;
        }
    }

    pub fn targets_of(&self, cc: Cc) -> impl Iterator<Item = (usize, &MapEntry)> {
        self.entries()
            .iter()
            .enumerate()
            .filter(move |(_, e)| e.cc == cc)
    }
}
