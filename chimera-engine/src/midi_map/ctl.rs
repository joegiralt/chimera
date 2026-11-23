// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! Controller maps (#807, ADR 0226, 0323): named bindings kept on the card,
//! apart from projects and Sounds, so a controller is set up once.
//!
//! A map is one flat list of at most `MAP_ROWS` rows, any map on any
//! engine: a row whose target the Part's engine lacks does nothing. A row
//! is 8 B, the `Bindings` v1 row (`codec`), decoded on use; a SysEx row's
//! key sits in the map's key table, the row naming its slot.
//!
//! On the card: `/CHIMERA/MAPS/<stem>.A|B`, kind `Controller`, the map's
//! name in the header, then `MapRows` records, each a `Bindings` payload
//! of up to 16 rows, the map's rows in file order. An old sectioned map
//! (`MapSection`, ADR 0226) loads flattened. The stem comes from the name
//! (`stem`), so a project names its map and a load needs no listing.
//! Bank 2's, beside the codec: read and written whole through bank 1's
//! `read_small`, `write_small` and `list_stems` into the caller's
//! `FileBuf`, never a buffer on the stack.

use chimera_hal::store::{Dir, FileName, Store, StoreError};

use super::codec::{self, MapRecord, input_bytes, input_of};
pub use super::stem;
use super::{Binding, Feedback, Input, MAP_LEN, MapFull, Mappable, MidiMap, Span};
use crate::name::SoundName;
use crate::storage::{
    Crc32, FileError, FileKind, Generation, HEADER_LEN, Header, LoadError, Ready, RecordBuf,
    RecordTag, SaveError, Side, TRAILER_LEN, addr_code, addr_of, list_stems, peek_header,
    read_small, write_small,
};

/// The most rows a map holds (ADR 0323).
pub const MAP_ROWS: usize = 90;
/// The SysEx keys a map holds: a Sound's whole map of them fits.
pub const SX_KEYS: usize = MAP_LEN;
/// The maps a listing holds.
pub const MAX_MAPS: usize = 16;
/// A file's records: 16 rows each.
const CHUNKS: usize = MAP_ROWS.div_ceil(MAP_LEN);
/// An old map's sections (ADR 0226): shared, then one an engine.
const SECTIONS: u8 = 5;

const _: () = assert!(MAP_ROWS <= u8::MAX as usize);

/// The control's tag in a row's first byte's top three bits (`codec`).
const TAG_SYSEX: u8 = 4;
const ECHO: u8 = 1;

/// A row's place in a map, below `MAP_ROWS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MapRowIx(u8);

impl MapRowIx {
    pub const fn new(i: usize) -> Option<Self> {
        if i < MAP_ROWS {
            Some(Self(i as u8))
        } else {
            None
        }
    }

    pub const fn get(self) -> usize {
        self.0 as usize
    }
}

/// A map's row count, 0 to `MAP_ROWS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MapLen(u8);

impl MapLen {
    pub const ZERO: MapLen = MapLen(0);
    pub const FULL: MapLen = MapLen(MAP_ROWS as u8);

    pub const fn new(n: usize) -> Option<Self> {
        if n <= MAP_ROWS {
            Some(Self(n as u8))
        } else {
            None
        }
    }

    pub const fn get(self) -> usize {
        self.0 as usize
    }
}

/// One row as the card's `Bindings` v1 row: `[tag|kind] [hi] [lo] [block]
/// [param] [min] [max] [flags]`; a SysEx row's `lo` is its key's slot.
#[derive(Clone, Copy, PartialEq, Eq)]
struct PackedRow([u8; 8]);

impl PackedRow {
    fn sysex(self) -> bool {
        self.0[0] >> 5 == TAG_SYSEX
    }

    fn target(self) -> [u8; 2] {
        [self.0[3], self.0[4]]
    }
}

/// A controller map: its rows and its SysEx keys.
#[derive(Clone, Copy)]
pub struct ControllerMap {
    rows: [PackedRow; MAP_ROWS],
    keys: [u32; SX_KEYS],
    len: MapLen,
}

/// Equal by the rows as they decode: slots past `len` and key slots are
/// scratch.
impl PartialEq for ControllerMap {
    fn eq(&self, o: &Self) -> bool {
        self.len == o.len && (0..self.len()).all(|i| self.get(i) == o.get(i))
    }
}

impl Eq for ControllerMap {}

impl core::fmt::Debug for ControllerMap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries((0..self.len()).map(|i| self.get(i)))
            .finish()
    }
}

/// An empty map built at `p`: a few stores, bank 2's, not a static's
/// image (ADR 0251).
///
/// # Safety
/// `p` is valid for a write of `ControllerMap`.
#[inline(never)]
pub unsafe fn empty_at(p: *mut ControllerMap) {
    // SAFETY: every field is plain bytes; all zero is an empty map.
    unsafe { core::ptr::write_bytes(p, 0, 1) };
}

impl ControllerMap {
    pub const EMPTY: Self = Self {
        rows: [PackedRow([0; 8]); MAP_ROWS],
        keys: [0; SX_KEYS],
        len: MapLen::ZERO,
    };

    pub const fn len(&self) -> usize {
        self.len.get()
    }

    pub const fn map_len(&self) -> MapLen {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len.0 == 0
    }

    /// Row `i`, if it is one this firmware reads.
    pub fn get(&self, i: usize) -> Option<Binding> {
        let r = self.rows.get(..self.len())?.get(i)?.0;
        let sx = PackedRow(r).sysex();
        let key = if sx {
            Some(*self.keys.get(usize::from(r[2]))?)
        } else {
            None
        };
        let input = input_of(r[0], r[1], if sx { 0 } else { r[2] }, key)?;
        let target = addr_of(r[3], r[4]).and_then(Mappable::new)?;
        Some(Binding {
            input,
            target,
            span: Span {
                min: r[5],
                max: r[6],
            },
            feedback: if r[7] & ECHO != 0 {
                Feedback::Value
            } else {
                Feedback::Off
            },
        })
    }

    /// Row `i`'s control and how it is read, with no decode of its
    /// target: a message's scan of the map.
    pub fn input(&self, i: usize) -> Option<Input> {
        let r = self.rows.get(..self.len())?.get(i)?.0;
        if PackedRow(r).sysex() {
            let key = *self.keys.get(usize::from(r[2]))?;
            return input_of(r[0], r[1], 0, Some(key));
        }
        input_of(r[0], r[1], r[2], None)
    }

    /// The rows, in order.
    pub fn rows(&self) -> impl Iterator<Item = Binding> + '_ {
        (0..self.len()).filter_map(|i| self.get(i))
    }

    /// The row on target `t`.
    pub fn position(&self, t: Mappable) -> Option<usize> {
        let code = addr_code(t.addr());
        self.rows[..self.len()]
            .iter()
            .position(|r| r.target() == code)
    }

    /// A key slot for `k`: one a row other than `except` holds it in,
    /// else one no such row uses.
    fn key_slot(&mut self, k: u32, except: Option<usize>) -> Option<u8> {
        let used = |m: &Self, slot: usize| {
            (0..m.len()).any(|i| {
                Some(i) != except && m.rows[i].sysex() && usize::from(m.rows[i].0[2]) == slot
            })
        };
        if let Some(s) = (0..SX_KEYS).find(|&s| used(self, s) && self.keys[s] == k) {
            return Some(s as u8);
        }
        let s = (0..SX_KEYS).find(|&s| !used(self, s))?;
        self.keys[s] = k;
        Some(s as u8)
    }

    /// `b` packed as row `at` would hold it.
    fn pack(&mut self, b: &Binding, at: Option<usize>) -> Result<PackedRow, MapFull> {
        let mut i = input_bytes(b.input);
        if let Input::SysEx(k) = b.input {
            i[2] = self.key_slot(k.key(), at).ok_or(MapFull)?;
        }
        let a = addr_code(b.target.addr());
        let echo = match b.feedback {
            Feedback::Value => ECHO,
            Feedback::Off => 0,
        };
        Ok(PackedRow([
            i[0], i[1], i[2], a[0], a[1], b.span.min, b.span.max, echo,
        ]))
    }

    /// `b` appended, or over the row on its target (a param appears
    /// once): where it went. `MapFull` past `MAP_ROWS` rows or SysEx keys.
    pub fn push(&mut self, b: Binding) -> Result<MapRowIx, MapFull> {
        if let Some(i) = self.position(b.target) {
            self.set(i, b)?;
            return MapRowIx::new(i).ok_or(MapFull);
        }
        let n = self.len();
        if n >= MAP_ROWS {
            return Err(MapFull);
        }
        self.rows[n] = self.pack(&b, None)?;
        self.len = MapLen(n as u8 + 1);
        MapRowIx::new(n).ok_or(MapFull)
    }

    /// Row `i` made `b`; out of range does nothing.
    pub fn set(&mut self, i: usize, b: Binding) -> Result<(), MapFull> {
        if i < self.len() {
            self.rows[i] = self.pack(&b, Some(i))?;
        }
        Ok(())
    }

    /// Row `i` deleted, the order kept; out of range does nothing.
    pub fn remove(&mut self, i: usize) {
        let n = self.len();
        if i < n {
            self.rows.copy_within(i + 1..n, i);
            self.len = MapLen(n as u8 - 1);
        }
    }

    /// Row `b`, by its control and target.
    pub fn find(&self, b: &Binding) -> Option<usize> {
        let i = self.position(b.target)?;
        self.get(i)
            .is_some_and(|x| x.input.same_control(&b.input))
            .then_some(i)
    }

    /// Row `b` (`find`) made `to`, or dropped with `None`. `false`: none.
    pub fn change(&mut self, b: &Binding, to: Option<Binding>) -> Result<bool, MapFull> {
        let Some(i) = self.find(b) else {
            return Ok(false);
        };
        match to {
            Some(to) => self.set(i, to)?,
            None => self.remove(i),
        }
        Ok(true)
    }

    /// `own`, a Part's Sound rows, saved into the map: each clears the
    /// rows on its control or target, then is appended. `MapFull` past
    /// `MAP_ROWS` rows, the map as it was.
    pub fn absorb(&mut self, own: &MidiMap) -> Result<(), MapFull> {
        let mut next = *self;
        for r in own.entries() {
            let mut i = 0;
            while i < next.len() {
                match next.get(i) {
                    Some(x) if x.target == r.target || x.input.same_control(&r.input) => {
                        next.remove(i)
                    }
                    _ => i += 1,
                }
            }
        }
        for r in own.entries() {
            next.push(*r)?;
        }
        *self = next;
        Ok(())
    }
}

/// The largest map file: the header, six records of 16 SysEx rows, the
/// trailer. An old sectioned map (at most 706 B) fits too.
pub const FILE_MAX: usize = HEADER_LEN + CHUNKS * (4 + 1 + MAP_LEN * 12) + TRAILER_LEN;

/// A map file's bytes, read or written whole: the caller's, never the
/// stack's (ADR 0025).
pub struct FileBuf([u8; FILE_MAX]);

impl FileBuf {
    pub const fn new() -> Self {
        Self([0; FILE_MAX])
    }
}

impl Default for FileBuf {
    fn default() -> Self {
        Self::new()
    }
}

/// Side `side` of stem `st`.
fn side_file(st: &[u8], side: Side) -> Option<FileName> {
    FileName::new(Dir::Maps, st, side.ext())
}

/// A `Bindings` payload's rows appended to `out`: the rows skipped.
fn take_rows(p: &[u8], out: &mut ControllerMap) -> Option<u16> {
    let mut chunk = core::mem::MaybeUninit::<MidiMap>::uninit();
    // SAFETY: `empty_at` writes the whole map: no image of it copied.
    let chunk = unsafe {
        MidiMap::empty_at(chunk.as_mut_ptr());
        chunk.assume_init_mut()
    };
    let mut lost = codec::decode(MapRecord::Bindings, p, Some(chunk)).ok()?;
    for e in chunk.entries() {
        if out.push(*e).is_err() {
            lost += 1;
        }
    }
    Some(lost)
}

/// A side's bytes judged: its header (kind `Controller`, CRC good), its
/// records walked onto `out` if given, the rows skipped. `None`: not a map
/// this firmware reads.
fn parse(b: &[u8], mut out: Option<&mut ControllerMap>) -> Option<(Header, u16)> {
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
    if let Some(o) = out.as_deref_mut() {
        // SAFETY: `o` is a map's place.
        unsafe { empty_at(o) };
    }
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
        let rows = match RecordTag::from_code(tag) {
            Some(RecordTag::MapRows) => p,
            Some(RecordTag::MapSection) => {
                // An old map's section: its rows join the list in file order.
                let (&code, rest) = p.split_first()?;
                if code >= SECTIONS {
                    continue;
                }
                if seen & 1 << code != 0 {
                    return None;
                }
                seen |= 1 << code;
                rest
            }
            // An unknown must-understand record: not ours to read.
            None if tag & crate::storage::CRITICAL != 0 => return None,
            // Any other record: not a map's, skipped.
            Some(_) | None => continue,
        };
        match out.as_deref_mut() {
            Some(o) => lost += take_rows(rows, o)?,
            None => {
                codec::decode(MapRecord::Bindings, rows, None).ok()?;
            }
        }
    }
    Some((h, lost))
}

/// Of stem `st`'s two sides, the one a load takes (good, newest; with
/// `want`, of those naming it, its map into `want`'s): its header, the
/// rows skipped and its side. `Missing` with none.
fn load_stem(
    s: &mut dyn Store,
    r: &Ready,
    st: &[u8],
    buf: &mut FileBuf,
    want: Option<(&SoundName, &mut ControllerMap)>,
) -> Result<(Header, u16, Side), LoadError> {
    let mut best: Option<(Header, u16, Side)> = None;
    // The side whose bytes `buf` holds now, and how many.
    let mut held: Option<(Side, usize)> = None;
    let name = want.as_ref().map(|(n, _)| **n);
    for side in [Side::A, Side::B] {
        let f = side_file(st, side).ok_or(LoadError::Missing)?;
        held = None;
        let n = match read_small(s, r.volume(), f, &mut buf.0) {
            Ok(n) => n,
            Err(StoreError::NotFound | StoreError::Corrupt) => continue,
            Err(e) => return Err(LoadError::Store(e)),
        };
        held = Some((side, n));
        if let Some((h, lost)) = parse(&buf.0[..n], None)
            && best.is_none_or(|(b, ..)| h.generation.is_newer_than(b.generation))
            && name.is_none_or(|w| h.name == Some(w))
        {
            best = Some((h, lost, side));
        }
    }
    let (h, _, side) = best.ok_or(LoadError::Missing)?;
    let Some((_, out)) = want else {
        return best.ok_or(LoadError::Missing);
    };
    let n = match held {
        Some((s0, n)) if s0 == side => n,
        _ => {
            let f = side_file(st, side).ok_or(LoadError::Missing)?;
            read_small(s, r.volume(), f, &mut buf.0).map_err(LoadError::Store)?
        }
    };
    // The bytes judged above, read again: changed since, the card's fault.
    let (_, lost) = parse(&buf.0[..n], Some(out)).ok_or(LoadError::File(FileError::Corrupt))?;
    Ok((h, lost, side))
}

/// Map `n` from the card into `out`, through `buf`: the rows skipped. A
/// file whose header names another map is `Missing`, `out` untouched.
pub fn load(
    s: &mut dyn Store,
    r: &Ready,
    n: &SoundName,
    buf: &mut FileBuf,
    out: &mut ControllerMap,
) -> Result<u16, LoadError> {
    let (st, len) = stem(n);
    load_stem(s, r, &st[..len], buf, Some((n, out))).map(|(_, lost, _)| lost)
}

/// `m` to the card as map `n`, through `buf`: the side a load doesn't
/// take, a generation on, so a cut write leaves the last map loadable. A
/// stem holding another map's name is refused (`BadName`), left as it is.
pub fn save(
    s: &mut dyn Store,
    r: &Ready,
    n: &SoundName,
    buf: &mut FileBuf,
    m: &ControllerMap,
) -> Result<(), SaveError> {
    s.make_dir(r.volume(), Dir::Chimera)?;
    s.make_dir(r.volume(), Dir::Maps)?;
    let (st, len) = stem(n);
    let st = &st[..len];
    let (side, generation) = match load_stem(s, r, st, buf, None) {
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
    let at = frame(&mut buf.0, &h, m).ok_or(SaveError::File(FileError::Bounds))?;
    let f = side_file(st, side).ok_or(StoreError::Io)?;
    write_small(s, r.volume(), f, buf.0.get(..at).unwrap_or_default())?;
    Ok(())
}

/// `m`'s file with header `h` into `b`: its length. Out of line: its
/// record buffer is not in a caller's frame (ADR 0025's stack check).
#[inline(never)]
fn frame(b: &mut [u8], h: &Header, m: &ControllerMap) -> Option<usize> {
    let mut at = 0;
    put(b, &mut at, &h.encode())?;
    let mut i = 0;
    while i < m.len() {
        let mut chunk = core::mem::MaybeUninit::<MidiMap>::uninit();
        // SAFETY: `empty_at` writes the whole map: no image of it copied.
        let chunk = unsafe {
            MidiMap::empty_at(chunk.as_mut_ptr());
            chunk.assume_init_mut()
        };
        while i < m.len() && chunk.len() < MAP_LEN {
            // A row this firmware can't read was never let in.
            if let Some(e) = m.get(i)
                && chunk.learn(e.input, e.target).is_ok()
            {
                chunk.set(chunk.len() - 1, e);
            }
            i += 1;
        }
        let mut p = RecordBuf::new();
        codec::encode(chunk, &mut p);
        let p = p.as_slice();
        put(b, &mut at, &RecordTag::MapRows.code().to_le_bytes())?;
        put(b, &mut at, &(p.len() as u16).to_le_bytes())?;
        put(b, &mut at, p)?;
    }
    let mut crc = Crc32::new();
    crc.update(b.get(..at)?);
    put(b, &mut at, &crc.finish().to_le_bytes())?;
    Some(at)
}

/// `x` into `b` at `at`, which moves past it; `None` past `b`'s end.
fn put(b: &mut [u8], at: &mut usize, x: &[u8]) -> Option<()> {
    b.get_mut(*at..*at + x.len())?.copy_from_slice(x);
    *at += x.len();
    Some(())
}

/// Map `n`'s generation as the card holds it, the side a load takes;
/// `None` if no file names it. A copy read at one generation and the
/// card's at another: the card changed since (a console `put`).
pub fn generation(
    s: &mut dyn Store,
    r: &Ready,
    n: &SoundName,
    buf: &mut FileBuf,
) -> Result<Option<Generation>, LoadError> {
    let (st, len) = stem(n);
    match load_stem(s, r, &st[..len], buf, None) {
        Ok((h, ..)) if h.name == Some(*n) => Ok(Some(h.generation)),
        Ok(_) | Err(LoadError::Missing) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Whether a map holds `n`'s stem on the card: `n`, or one sharing it.
pub fn taken(
    s: &mut dyn Store,
    r: &Ready,
    n: &SoundName,
    buf: &mut FileBuf,
) -> Result<bool, LoadError> {
    let (st, len) = stem(n);
    match load_stem(s, r, &st[..len], buf, None) {
        Ok(_) => Ok(true),
        Err(LoadError::Missing) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Map `n`'s pair deleted from the card, the side a load does not take
/// first. `Missing`: no file names it.
pub fn delete(
    s: &mut dyn Store,
    r: &Ready,
    n: &SoundName,
    buf: &mut FileBuf,
) -> Result<(), LoadError> {
    let (st, len) = stem(n);
    let st = &st[..len];
    let live = match load_stem(s, r, st, buf, None) {
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

/// The maps on the card, by name in order, into `out`: how many, and
/// whether the card holds more than `out` (of the first `MAX_MAPS + 1`
/// stems listed). No folder is no maps; a file that won't load is left
/// out. Inlined: its callers, the MIDI UI's card work and the editor's
/// (ADR 0289), are in two bank-2 groups.
#[inline(always)]
pub fn list(
    s: &mut dyn Store,
    r: &Ready,
    buf: &mut FileBuf,
    out: &mut [Option<SoundName>; MAX_MAPS],
) -> Result<(usize, bool), StoreError> {
    let mut stems = [([0u8; 8], 0u8); MAX_MAPS + 1];
    let n = list_stems(s, r.volume(), Dir::Maps, &[b"A", b"B"], &mut stems)?;
    let (mut k, mut more) = (0, false);
    for (b, l) in &stems[..n] {
        match load_stem(s, r, &b[..usize::from(*l)], buf, None) {
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
    use crate::block::ParamId;
    use crate::dsp::algo::params::AlgoParams;
    use crate::params::FilterParams;

    fn row(cc: u8, b: BlockRef, p: ParamId) -> Binding {
        Binding {
            input: Input::cc(cc).unwrap(),
            target: Mappable::new(ParamAddr::new(b, p)).unwrap(),
            span: Span::FULL,
            feedback: Feedback::Off,
        }
    }

    #[test]
    fn a_row_round_trips_packed() {
        let mut m = ControllerMap::EMPTY;
        let b = Binding {
            span: Span { min: 9, max: 200 },
            feedback: Feedback::Value,
            ..row(20, BlockRef::Algo, AlgoParams::MORPH)
        };
        assert_eq!(m.push(b), Ok(MapRowIx(0)));
        assert_eq!(m.get(0), Some(b));
    }

    #[test]
    fn push_replaces_a_target_in_place() {
        let mut m = ControllerMap::EMPTY;
        m.push(row(20, BlockRef::Filter, FilterParams::CUTOFF))
            .unwrap();
        m.push(row(22, BlockRef::Filter, FilterParams::RESONANCE))
            .unwrap();
        m.push(row(21, BlockRef::Filter, FilterParams::CUTOFF))
            .unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(
            m.get(0),
            Some(row(21, BlockRef::Filter, FilterParams::CUTOFF))
        );
    }

    #[test]
    fn row_bounds() {
        assert!(MapRowIx::new(89).is_some());
        assert!(MapRowIx::new(90).is_none());
        assert!(MapLen::new(90).is_some());
        assert!(MapLen::new(91).is_none());
        assert_eq!(MapLen::FULL.get(), 90);
    }
}
