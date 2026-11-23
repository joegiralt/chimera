// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Controller maps (#807, ADR 0226): named bindings kept on the card,
//! apart from projects and Sounds, so a controller is set up once.
//!
//! A map holds a shared section and one section an engine. A binding's
//! section is its target's engine; a target every engine reads is shared.
//! A Part on an engine plays that engine's rows, then the shared rows on
//! other controls: its layer, at most `MAP_LEN` rows. The Part's Sound's
//! own rows win a control over the layer.
//!
//! On the card: `/CHIMERA/MAPS/<stem>.A|B`, kind `Controller`, the map's
//! name in the header, then a `MapSection` record a non-empty section: its
//! code (0 shared, 1 + the engine's disk code), then a `Bindings` payload
//! (`codec`). The stem comes from the name (`stem`), so a project names
//! its map and a load needs no listing. Bank 2's, beside the codec: its
//! file is small, read and written whole through bank 1's `read_small`,
//! `write_small` and `list_stems` and framed here, so bank 2 builds no
//! trait object of its own (an anonymous vtable no glob places).

use chimera_hal::store::{Dir, FileName, Store, StoreError};

use super::codec::{self, MapRecord};
pub use super::stem;
use super::{Binding, MAP_LEN, MapFull, MidiMap};
use crate::block::DiskCode;
use crate::name::SoundName;
use crate::params::EngineType;
use crate::storage::{
    Crc32, FileError, FileKind, Generation, HEADER_LEN, Header, LoadError, Ready, RecordBuf,
    RecordTag, SaveError, Side, TRAILER_LEN, list_stems, peek_header, read_small, write_small,
};

/// The shared section and one an engine, DRUM aside.
pub const SECTIONS: usize = 5;

/// The maps a listing holds.
pub const MAX_MAPS: usize = 16;

/// Section `i`'s engine: `None` for 0, the shared one, and past the last.
fn engine_at(i: usize) -> Option<EngineType> {
    EngineType::from_disk_code(u8::try_from(i.checked_sub(1)?).ok()?).filter(|&e| at(Some(e)) == i)
}

/// The section of `e`'s rows; `None` is shared. DRUM has none of its own
/// (its knobs are its pads'): its rows are the shared ones.
fn at(e: Option<EngineType>) -> usize {
    match e {
        None | Some(EngineType::Drum) => 0,
        Some(e) => 1 + usize::from(e.disk_code()),
    }
}

/// A controller map: its sections, each a `MidiMap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControllerMap {
    sections: [MidiMap; SECTIONS],
}

/// An empty map built at `p`: a few stores, bank 2's, not a static's
/// image (ADR 0251).
///
/// # Safety
/// `p` is valid for a write of `ControllerMap`.
#[inline(never)]
pub unsafe fn empty_at(p: *mut ControllerMap) {
    for i in 0..SECTIONS {
        // SAFETY: section `i` of the caller's `*p`.
        unsafe {
            MidiMap::empty_at(
                core::ptr::addr_of_mut!((*p).sections)
                    .cast::<MidiMap>()
                    .add(i),
            )
        };
    }
}

impl ControllerMap {
    pub const EMPTY: Self = Self {
        sections: [MidiMap::EMPTY; SECTIONS],
    };

    /// `e`'s section; `None` is the shared one.
    pub fn section(&self, e: Option<EngineType>) -> &MidiMap {
        &self.sections[at(e)]
    }

    pub fn is_empty(&self) -> bool {
        self.sections.iter().all(MidiMap::is_empty)
    }

    /// What a Part on `e` plays: `e`'s rows, then the shared rows on other
    /// controls, at most `MAP_LEN`; and how many shared rows didn't fit.
    pub fn layer(&self, e: EngineType) -> (MidiMap, u16) {
        let mut out = *self.section(Some(e));
        let mut lost = 0;
        for r in self.sections[0].entries() {
            if out.entries().iter().any(|o| o.input.same_control(&r.input)) {
                continue;
            }
            if !push(&mut out, *r) {
                lost += 1;
            }
        }
        (out, lost)
    }

    /// `own`, a Part on `e`'s Sound rows, saved into the map as the Part
    /// plays them: each row clears the rows on its control or target in
    /// its target's section, a shared row in `e`'s too (an engine's row
    /// wins a layer), then joins its section. `MapFull` when a section or
    /// an engine's layer would pass `MAP_LEN`; the map stays.
    pub fn absorb(&mut self, own: &MidiMap, e: EngineType) -> Result<(), MapFull> {
        let mut next = *self;
        for r in own.entries() {
            let mine = at(r.target.engine());
            clear(&mut next.sections[mine], r);
            if mine == 0 {
                clear(&mut next.sections[at(Some(e))], r);
            }
        }
        for r in own.entries() {
            if !push(&mut next.sections[at(r.target.engine())], *r) {
                return Err(MapFull);
            }
        }
        for e in EngineType::ALL {
            if next.layer(e).1 != 0 {
                return Err(MapFull);
            }
        }
        *self = next;
        Ok(())
    }

    /// Where row `b` that a Part on `e` plays is, by its control and
    /// target: in `e`'s section, else the shared one.
    fn find(&self, b: &Binding, e: EngineType) -> Option<(usize, usize)> {
        [at(Some(e)), 0].into_iter().find_map(|i| {
            self.sections[i]
                .entries()
                .iter()
                .position(|x| x.target == b.target && x.input.same_control(&b.input))
                .map(|j| (i, j))
        })
    }

    /// Row `b` that a Part on `e` plays (`find`) made `to`, or dropped
    /// with `None`, in its section. `false`: none. One method for both, so
    /// a caller's two jobs make one call, nothing the outliner can share
    /// into bank 1.
    pub fn change(&mut self, b: &Binding, to: Option<Binding>, e: EngineType) -> bool {
        let Some((i, j)) = self.find(b, e) else {
            return false;
        };
        match to {
            Some(to) => self.sections[i].set(j, to),
            None => self.sections[i].remove(j),
        }
        true
    }
}

/// `r` appended to `m`; `false` when `m` is full.
fn push(m: &mut MidiMap, r: Binding) -> bool {
    if m.len() >= MAP_LEN || m.learn(r.input, r.target).is_err() {
        return false;
    }
    m.set(m.len() - 1, r);
    true
}

/// `m` without the rows on `r`'s control or target.
fn clear(m: &mut MidiMap, r: &Binding) {
    let mut i = 0;
    while i < m.len() {
        let x = m.entries()[i];
        if x.target == r.target || x.input.same_control(&r.input) {
            m.remove(i);
        } else {
            i += 1;
        }
    }
}

/// The largest map file: the header, five full sections of 8-B rows, the
/// trailer. SysEx rows are 12 B (#846): a map of many past it is refused
/// (`fits`), never cut, so the buffers on the stack stay this size.
const FILE_MAX: usize = HEADER_LEN + SECTIONS * (4 + 2 + MAP_LEN * 8) + TRAILER_LEN;

/// `m`'s file fits `FILE_MAX`: its header, each non-empty section's tag,
/// length, code and payload, and its trailer.
pub fn fits(m: &ControllerMap) -> bool {
    let body: usize = m
        .sections
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| 4 + 1 + codec::encoded_len(s))
        .sum();
    HEADER_LEN + body + TRAILER_LEN <= FILE_MAX
}

/// Side `side` of stem `st`.
fn side_file(st: &[u8], side: Side) -> Option<FileName> {
    FileName::new(Dir::Maps, st, side.ext())
}

/// A side's bytes judged: its header (kind `Controller`, CRC good), its
/// records walked onto `out` (left part written on `None`), the bindings
/// skipped. `None`: not a map this firmware reads.
fn parse(b: &[u8], out: &mut ControllerMap) -> Option<(Header, u16)> {
    let body = b.len().checked_sub(TRAILER_LEN)?;
    let head = <&[u8; HEADER_LEN]>::try_from(b.get(..HEADER_LEN)?).ok()?;
    let h = peek_header(head)
        .ok()
        .filter(|h| h.kind == FileKind::Controller)?;
    let mut crc = Crc32::new();
    crc.update(b.get(..body)?);
    if crc.finish().to_le_bytes() != *b.get(body..)? {
        return None;
    }
    // SAFETY: `out` is a map's place; every field is `Copy`.
    unsafe { empty_at(out) };
    let (mut at, mut lost, mut seen) = (HEADER_LEN, 0u16, 0u8);
    while at < body {
        let r = b.get(at..at + 4)?;
        let (tag, len) = (
            u16::from_le_bytes([r[0], r[1]]),
            usize::from(u16::from_le_bytes([r[2], r[3]])),
        );
        let p = b
            .get(at + 4..at + 4 + len)
            .filter(|_| at + 4 + len <= body)?;
        at += 4 + len;
        if tag != RecordTag::MapSection.code() {
            // An unknown must-understand record: not ours to read.
            if tag & crate::storage::CRITICAL != 0 {
                return None;
            }
            continue;
        }
        let (&code, rest) = p.split_first()?;
        let i = usize::from(code);
        if i != 0 && engine_at(i).is_none() {
            continue;
        }
        if seen & 1 << i != 0 {
            return None;
        }
        seen |= 1 << i;
        let sec = out.sections.get_mut(i)?;
        lost += codec::decode(MapRecord::Bindings, rest, Some(sec)).ok()?;
    }
    Some((h, lost))
}

/// Of stem `st`'s two sides, the one a load takes (good, newest; with
/// `want`, of those naming it, its map into `want`'s): its header, the
/// bindings skipped and its side. Each side read once. `Missing` with
/// none.
fn load_stem(
    s: &mut dyn Store,
    r: &Ready,
    st: &[u8],
    mut want: Option<(&SoundName, &mut ControllerMap)>,
) -> Result<(Header, u16, Side), LoadError> {
    let mut buf = [0u8; FILE_MAX];
    let mut staged = core::mem::MaybeUninit::uninit();
    // SAFETY: `empty_at` writes the whole map.
    let staged = unsafe {
        empty_at(staged.as_mut_ptr());
        staged.assume_init_mut()
    };
    let mut best: Option<(Header, u16, Side)> = None;
    for side in [Side::A, Side::B] {
        let f = side_file(st, side).ok_or(LoadError::Missing)?;
        let n = match read_small(s, r.volume(), f, &mut buf) {
            Ok(n) => n,
            Err(StoreError::NotFound | StoreError::Corrupt) => continue,
            Err(e) => return Err(LoadError::Store(e)),
        };
        if let Some((h, lost)) = parse(&buf[..n], staged)
            && best.is_none_or(|(b, ..)| h.generation.is_newer_than(b.generation))
        {
            match want.as_mut() {
                Some((w, _)) if h.name != Some(**w) => continue,
                Some((_, o)) => **o = *staged,
                None => {}
            }
            best = Some((h, lost, side));
        }
    }
    best.ok_or(LoadError::Missing)
}

/// Map `n` from the card into `out`: the bindings skipped. A file whose
/// header names another map is `Missing`, `out` untouched.
pub fn load(
    s: &mut dyn Store,
    r: &Ready,
    n: &SoundName,
    out: &mut ControllerMap,
) -> Result<u16, LoadError> {
    let (st, len) = stem(n);
    load_stem(s, r, &st[..len], Some((n, out))).map(|(_, lost, _)| lost)
}

/// `m` to the card as map `n`: the side a load doesn't take, a generation
/// on, so a cut write leaves the last map loadable. A stem holding
/// another map's name is refused (`BadName`), left as it is.
pub fn save(
    s: &mut dyn Store,
    r: &Ready,
    n: &SoundName,
    m: &ControllerMap,
) -> Result<(), SaveError> {
    // Past the file: refused before the card is touched (#846).
    if !fits(m) {
        return Err(SaveError::File(FileError::Bounds));
    }
    s.make_dir(r.volume(), Dir::Chimera)?;
    s.make_dir(r.volume(), Dir::Maps)?;
    let (st, len) = stem(n);
    let st = &st[..len];
    let (side, generation) = match load_stem(s, r, st, None) {
        Ok((h, ..)) if h.name != Some(*n) => return Err(SaveError::File(FileError::BadName)),
        Ok((h, _, side)) => (side.other(), h.generation.next()),
        Err(LoadError::Store(e)) => return Err(e.into()),
        Err(_) => (Side::A, Generation::FIRST),
    };
    let h = Header {
        kind: FileKind::Controller,
        generation,
        name: Some(*n),
    };
    write_side(s, r, st, side, &h, m)
}

/// Map `n`'s pair deleted from the card, the side a load does not take
/// first. `Missing`: no file names it.
pub fn delete(s: &mut dyn Store, r: &Ready, n: &SoundName) -> Result<(), LoadError> {
    let (st, len) = stem(n);
    let st = &st[..len];
    let live = match load_stem(s, r, st, None) {
        Ok((h, _, side)) if h.name == Some(*n) => side,
        Ok(_) => return Err(LoadError::Missing),
        Err(e) => return Err(e),
    };
    // ADR 0045's order: the side a load does not take first.
    for side in [live.other(), live] {
        let f = side_file(st, side).ok_or(LoadError::Missing)?;
        match s.delete(r.volume(), f) {
            Ok(()) | Err(StoreError::NotFound) => {}
            Err(e) => return Err(LoadError::Store(e)),
        }
    }
    Ok(())
}

/// `save`'s write of side `side`. Out of line: its buffers are not under
/// `load_stem`'s, nor in a caller's frame (ADR 0025's stack check).
#[inline(never)]
fn write_side(
    s: &mut dyn Store,
    r: &Ready,
    st: &[u8],
    side: Side,
    h: &Header,
    m: &ControllerMap,
) -> Result<(), SaveError> {
    let mut b = [0u8; FILE_MAX];
    let mut at = 0;
    let mut ok = put(&mut b, &mut at, &h.encode());
    for (i, sec) in m.sections.iter().enumerate() {
        if sec.is_empty() {
            continue;
        }
        let mut p = RecordBuf::new();
        p.u8(i as u8);
        codec::encode(sec, &mut p);
        let p = p.as_slice();
        ok &= put(&mut b, &mut at, &RecordTag::MapSection.code().to_le_bytes());
        ok &= put(&mut b, &mut at, &(p.len() as u16).to_le_bytes());
        ok &= put(&mut b, &mut at, p);
    }
    let mut crc = Crc32::new();
    crc.update(b.get(..at).unwrap_or_default());
    ok &= put(&mut b, &mut at, &crc.finish().to_le_bytes());
    if !ok {
        return Err(SaveError::File(FileError::Bounds));
    }
    let f = side_file(st, side).ok_or(StoreError::Io)?;
    write_small(s, r.volume(), f, b.get(..at).unwrap_or_default())?;
    Ok(())
}

/// `x` into `b` at `at`, which moves past it; `false`, nothing written,
/// past `b`'s end (`fits` refuses such a map first).
fn put(b: &mut [u8], at: &mut usize, x: &[u8]) -> bool {
    match b.get_mut(*at..*at + x.len()) {
        Some(to) => {
            to.copy_from_slice(x);
            *at += x.len();
            true
        }
        None => false,
    }
}

/// The maps on the card, by name in order, into `out`: how many, and
/// whether the card holds more than `out` (of the first `MAX_MAPS + 1`
/// stems listed). No folder is no maps; a file that won't load is left
/// out. Inlined: its callers, the MIDI UI's card work and the editor's
/// (ADR 0289), are in two bank-2 groups.
#[inline(always)]
pub fn list(
    s: &mut dyn Store,
    r: &Ready,
    out: &mut [Option<SoundName>; MAX_MAPS],
) -> Result<(usize, bool), StoreError> {
    let mut stems = [([0u8; 8], 0u8); MAX_MAPS + 1];
    let n = list_stems(s, r.volume(), Dir::Maps, &[b"A", b"B"], &mut stems)?;
    let (mut k, mut more) = (0, false);
    for (b, l) in &stems[..n] {
        match load_stem(s, r, &b[..usize::from(*l)], None) {
            Ok((h, ..)) => {
                if let Some(name) = h.name {
                    match out.get_mut(k) {
                        Some(o) => *o = Some(name),
                        None => more = true,
                    }
                    k = (k + 1).min(MAX_MAPS);
                }
            }
            Err(LoadError::Store(e)) => return Err(e),
            Err(_) => {}
        }
    }
    // Insertion sort: a slice sort's closure would be its own instance.
    for i in 1..k {
        let mut j = i;
        while j > 0 && out[j - 1].map(|x| x.padded()) > out[j].map(|x| x.padded()) {
            out.swap(j - 1, j);
            j -= 1;
        }
    }
    out[k..].fill(None);
    Ok((k, more))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addr::{BlockRef, ParamAddr};
    use crate::dsp::algo::params::AlgoParams;
    use crate::midi_map::Input;
    use crate::midi_map::Mappable;

    fn row(cc: u8) -> Binding {
        let mut m = MidiMap::EMPTY;
        let t = Mappable::new(ParamAddr::new(BlockRef::Algo, AlgoParams::MORPH)).unwrap();
        m.learn(Input::cc(cc).unwrap(), t).unwrap();
        m.entries()[0]
    }

    /// A file can bind one target in two sections, on two controls: the
    /// drop takes the row picked, not the other.
    #[test]
    fn a_drop_takes_the_row_picked() {
        let mut m = ControllerMap::EMPTY;
        push(&mut m.sections[0], row(22));
        push(&mut m.sections[at(Some(EngineType::Algo))], row(21));
        assert!(m.change(&row(22), None, EngineType::Algo));
        assert!(m.section(None).is_empty());
        assert_eq!(m.section(Some(EngineType::Algo)).entries(), &[row(21)]);
    }

    /// DRUM has no section of its own: a Part on it plays the shared rows,
    /// and a section coded past the last engine's stays a later
    /// firmware's.
    #[test]
    fn drum_plays_the_shared_section() {
        let mut m = ControllerMap::EMPTY;
        push(&mut m.sections[0], row(22));
        assert_eq!(m.section(Some(EngineType::Drum)).entries(), &[row(22)]);
        assert_eq!(m.layer(EngineType::Drum), (*m.section(None), 0));
        assert!(m.absorb(&MidiMap::EMPTY, EngineType::Drum).is_ok());
        assert_eq!(engine_at(SECTIONS), None);
        assert_eq!(engine_at(SECTIONS - 1), Some(EngineType::Va));
    }
}
