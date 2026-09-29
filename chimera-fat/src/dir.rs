//! The 32 B directory entry: 8.3 names only, and the LFN checksum that ties
//! a long-name run to its entry. Pure.

use crate::volume::FsKind;
use chimera_hal::store::{Dir, FileName};

pub const ENTRY: usize = 32;

const NAME_LEN: usize = 11;
const STEM_LEN: usize = 8;
const ATTR: usize = 11;
/// In a long-name slot: the checksum of the short name it belongs to.
const LFN_SUM: usize = 13;
const CREATE_TIME: usize = 14;
const CREATE_DATE: usize = 16;
const ACCESS_DATE: usize = 18;
const CLUSTER_HI: usize = 20;
const WRITE_TIME: usize = 22;
const WRITE_DATE: usize = 24;
const CLUSTER_LO: usize = 26;
const SIZE: usize = 28;

const END: u8 = 0x00;
const FREE: u8 = 0xE5;
const ATTR_LABEL: u8 = 0x08;
const ATTR_DIR: u8 = 0x10;
const ATTR_FILE: u8 = 0x20;
/// Read-only, hidden, system and label: a long-name slot.
const ATTR_LFN: u8 = 0x0F;
const ATTR_LFN_MASK: u8 = 0x3F;
/// 2026-01-01: (year - 1980) << 9 | month << 5 | day. There is no clock.
const STAMP_DATE: u16 = (2026 - 1980) << 9 | 1 << 5 | 1;
const STAMP_TIME: u16 = 0;

/// A space-padded 8.3 name as stored: stem, then extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShortName([u8; NAME_LEN]);

impl ShortName {
    fn padded(stem: &[u8], ext: &[u8]) -> Self {
        let mut n = [b' '; NAME_LEN];
        n[..stem.len()].copy_from_slice(stem);
        n[STEM_LEN..STEM_LEN + ext.len()].copy_from_slice(ext);
        Self(n)
    }

    /// A directory's own entry, `.`.
    pub const DOT: Self = Self(*b".          ");
    /// Its parent's, `..`.
    pub const DOT_DOT: Self = Self(*b"..         ");

    pub fn file(f: &FileName) -> Self {
        Self::padded(f.stem(), f.ext())
    }

    pub fn dir(d: Dir) -> Self {
        Self::padded(
            match d {
                Dir::Chimera => b"CHIMERA",
                Dir::Projects => b"PROJECTS",
                Dir::Sounds => b"SOUNDS",
            },
            b"",
        )
    }

    /// This name in `dir`, if `FileName` takes it: trailing spaces are
    /// padding, any other odd byte refuses it.
    pub fn to_file(&self, dir: Dir) -> Option<FileName> {
        fn trim(b: &[u8]) -> &[u8] {
            let n = b.iter().rposition(|&c| c != b' ').map_or(0, |i| i + 1);
            &b[..n]
        }
        let (stem, ext) = self.0.split_at(STEM_LEN);
        FileName::new(dir, trim(stem), trim(ext))
    }
}

/// A file or directory entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: ShortName,
    pub start: u32,
    pub len: u32,
}

/// What a directory slot holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// No entry here or after.
    End,
    Free,
    /// A long-name slot, with its short name's checksum.
    Lfn(u8),
    /// A label, a dot entry, or a name `FileName` refuses: never listed or
    /// matched.
    Other,
    File(Entry),
    Dir(Entry),
}

fn u16_at(b: &[u8; ENTRY], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn put16(b: &mut [u8; ENTRY], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

pub fn parse(raw: &[u8; ENTRY], kind: FsKind) -> Slot {
    let attr = raw[ATTR];
    match raw[0] {
        END => return Slot::End,
        FREE => return Slot::Free,
        _ if attr & ATTR_LFN_MASK == ATTR_LFN => return Slot::Lfn(raw[LFN_SUM]),
        _ if attr & ATTR_LABEL != 0 => return Slot::Other,
        _ => {}
    }
    let mut name = [0; NAME_LEN];
    name.copy_from_slice(&raw[..NAME_LEN]);
    let name = ShortName(name);
    // The directory is irrelevant: only whether the name is valid.
    if name.to_file(Dir::Chimera).is_none() {
        return Slot::Other;
    }
    let hi = match kind {
        FsKind::Fat16 => 0,
        FsKind::Fat32 => u32::from(u16_at(raw, CLUSTER_HI)),
    };
    let e = Entry {
        name,
        start: hi << 16 | u32::from(u16_at(raw, CLUSTER_LO)),
        len: u32::from_le_bytes([raw[SIZE], raw[SIZE + 1], raw[SIZE + 2], raw[SIZE + 3]]),
    };
    if attr & ATTR_DIR != 0 {
        Slot::Dir(e)
    } else {
        Slot::File(e)
    }
}

/// `e` as a file (attribute 0x20) or directory (0x10, length 0), stamped
/// 2026-01-01 00:00. FAT16 keeps the cluster's high half 0.
pub fn encode(e: &Entry, is_dir: bool, kind: FsKind, raw: &mut [u8; ENTRY]) {
    raw.fill(0);
    raw[..NAME_LEN].copy_from_slice(&e.name.0);
    raw[ATTR] = if is_dir { ATTR_DIR } else { ATTR_FILE };
    for at in [CREATE_DATE, ACCESS_DATE, WRITE_DATE] {
        put16(raw, at, STAMP_DATE);
    }
    for at in [CREATE_TIME, WRITE_TIME] {
        put16(raw, at, STAMP_TIME);
    }
    if kind == FsKind::Fat32 {
        put16(raw, CLUSTER_HI, (e.start >> 16) as u16);
    }
    put16(raw, CLUSTER_LO, e.start as u16);
    let len = if is_dir { 0 } else { e.len };
    raw[SIZE..].copy_from_slice(&len.to_le_bytes());
}

/// Marks the slot deleted.
pub fn free(raw: &mut [u8; ENTRY]) {
    raw[0] = FREE;
}

/// The checksum a long-name run carries of its short name (fatgen103).
pub fn lfn_checksum(name: &ShortName) -> u8 {
    name.0
        .iter()
        .fold(0u8, |sum, &c| sum.rotate_right(1).wrapping_add(c))
}
