//! A source's descriptor (spec § 1 What the arena holds): the 16 B at the
//! start of `held`. Bank 1's with `bank2` (storage and the pool read it),
//! as `read` re-exports it; `read`'s decode is bank 2's.

use chimera_hal::units::Bytes;

use crate::stream::units::{Frames, Packing};

/// What a source holds: one sample, a transwave's frames, or TRON's zones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Shape {
    Plain = 0,
    Transwave = 1,
    ZoneSet = 2,
}

impl Shape {
    pub fn from_code(c: u8) -> Option<Shape> {
        match c {
            0 => Some(Shape::Plain),
            1 => Some(Shape::Transwave),
            2 => Some(Shape::ZoneSet),
            _ => None,
        }
    }

    /// The largest `count` this shape takes.
    fn max_count(self) -> u8 {
        match self {
            Shape::Plain => 1,
            Shape::Transwave => 32,
            Shape::ZoneSet => MAX_ZONES,
        }
    }
}

/// The descriptor's bytes at the start of `held`.
pub const DESC_LEN: usize = 16;
/// The fewest frames a source holds: `MIN_LOOP_FRAMES`, so no model's
/// window is shorter (STRETCH TAPE Task 8, fix round 2). The importer
/// refuses a shorter file.
pub const MIN_SOURCE_FRAMES: u32 = 64;
/// One zone's entry in a `ZoneSet`'s layout, after the descriptor:
/// `start` (LE), `frames` (LE), `key`.
pub const ZONE_LAYOUT_LEN: usize = 9;
/// The most zones a `ZoneSet` holds.
pub const MAX_ZONES: u8 = 24;

/// `held`'s first 16 B: `shape`, `count`, `rate` (LE), `head_frames` (LE,
/// 0 when held whole), `heads`, then 7 reserved zero bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub shape: Shape,
    pub count: u8,
    pub rate: u16,
    /// A streamed `Plain`'s reach past each note (`HEAD_MS` at its rate);
    /// each head's own frames are its table entry's.
    pub head_frames: u32,
    /// A streamed `Plain`'s heads (slice 1 § 5.1); 0 when held whole, and
    /// on a `ZoneSet`, whose heads are its zones'.
    pub heads: u8,
}

impl Descriptor {
    pub fn to_bytes(&self) -> [u8; DESC_LEN] {
        let mut b = [0; DESC_LEN];
        (b[0], b[1]) = (self.shape as u8, self.count);
        b[2..4].copy_from_slice(&self.rate.to_le_bytes());
        b[4..8].copy_from_slice(&self.head_frames.to_le_bytes());
        b[8] = self.heads;
        b
    }

    /// `None` for a short slice, an unknown shape, a count outside its
    /// shape's `1..=max` (a `Plain` holds exactly 1), a rate outside
    /// 4 000..=48 000 (the pool spec's range), `head_frames` on a
    /// `Transwave`, a `Plain`'s `heads` and `head_frames` not set together,
    /// `heads` on a `ZoneSet`, or a set reserved byte.
    pub fn from_bytes(b: &[u8]) -> Option<Descriptor> {
        let b: &[u8; DESC_LEN] = b.get(..DESC_LEN)?.try_into().ok()?;
        let shape = Shape::from_code(b[0])?;
        let d = Descriptor {
            shape,
            count: b[1],
            rate: u16::from_le_bytes([b[2], b[3]]),
            head_frames: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            heads: b[8],
        };
        let heads_ok = match shape {
            Shape::Plain => (d.heads == 0) == (d.head_frames == 0),
            Shape::Transwave => d.heads == 0 && d.head_frames == 0,
            Shape::ZoneSet => d.heads == 0,
        };
        let ok = (1..=shape.max_count()).contains(&d.count)
            && (4_000..=48_000).contains(&d.rate)
            && heads_ok
            && b[9..].iter().all(|&x| x == 0);
        ok.then_some(d)
    }

    /// Where the samples start in `held`: after a `ZoneSet`'s layout.
    pub(super) fn data_start(&self) -> usize {
        match self.shape {
            Shape::ZoneSet => DESC_LEN + ZONE_LAYOUT_LEN * self.count as usize,
            _ => DESC_LEN,
        }
    }
}

/// A head's frames `first..end`: both even, `first < end`. The only way
/// a head table is written, so a head's span always starts on a 12-bit
/// pair (`SourceView::span`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadSpec {
    first: u32,
    end: u32,
}

impl HeadSpec {
    pub const fn new(first: u32, end: u32) -> Option<HeadSpec> {
        if first.is_multiple_of(2) && end.is_multiple_of(2) && first < end {
            Some(HeadSpec { first, end })
        } else {
            None
        }
    }

    pub const fn first(&self) -> u32 {
        self.first
    }

    pub const fn end(&self) -> u32 {
        self.end
    }

    /// Its packed samples.
    pub const fn bytes(&self) -> Bytes {
        Frames::new(self.end - self.first).to_bytes(Packing::Twelve)
    }

    /// `bytes`, as an index into `held`.
    pub const fn byte_len(&self) -> usize {
        self.bytes().get() as usize
    }

    fn to_bytes(self) -> [u8; HEAD_ENTRY] {
        let mut b = [0; HEAD_ENTRY];
        b[..4].copy_from_slice(&self.first.to_le_bytes());
        b[4..].copy_from_slice(&self.end.to_le_bytes());
        b
    }

    fn from_bytes(b: &[u8; HEAD_ENTRY]) -> Option<HeadSpec> {
        let w = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        HeadSpec::new(w(0), w(4))
    }
}

/// A head table's entry: `first`, then `end` (LE).
pub const HEAD_ENTRY: usize = 8;

/// A streamed `Plain`'s `held`: the descriptor, a `HEAD_ENTRY` a head,
/// then each head's samples in turn.
pub fn streamed_held_len(heads: &[HeadSpec]) -> usize {
    DESC_LEN + HEAD_ENTRY * heads.len() + heads.iter().map(HeadSpec::byte_len).sum::<usize>()
}

/// Writes `d` (`heads` set to their count) and the table into `out`;
/// where the first head's samples go. `None` if `out` is shorter, or past
/// 255 heads.
pub fn write_head_table(d: Descriptor, heads: &[HeadSpec], out: &mut [u8]) -> Option<usize> {
    let d = Descriptor {
        heads: u8::try_from(heads.len()).ok()?,
        ..d
    };
    let end = DESC_LEN + HEAD_ENTRY * heads.len();
    let out = out.get_mut(..end)?;
    out[..DESC_LEN].copy_from_slice(&d.to_bytes());
    let (table, _) = out[DESC_LEN..].as_chunks_mut::<HEAD_ENTRY>();
    for (h, o) in heads.iter().zip(table) {
        *o = h.to_bytes();
    }
    Some(end)
}

/// A streamed `Plain`'s head table, after the descriptor, every entry a
/// `HeadSpec` and every head's samples in `held`: checked once, here.
#[derive(Clone, Copy, Debug)]
pub struct HeadTable<'a> {
    entries: &'a [[u8; HEAD_ENTRY]],
}

impl HeadTable<'_> {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Head `i`, and where its samples start in `held`.
    pub fn get(&self, i: usize) -> Option<(HeadSpec, usize)> {
        let h = HeadSpec::from_bytes(self.entries.get(i)?)?;
        let before: usize = self.entries[..i]
            .iter()
            .filter_map(HeadSpec::from_bytes)
            .map(|h| h.byte_len())
            .sum();
        Some((h, DESC_LEN + HEAD_ENTRY * self.len() + before))
    }

    pub fn iter(&self) -> impl Iterator<Item = HeadSpec> + '_ {
        self.entries.iter().filter_map(HeadSpec::from_bytes)
    }
}

/// `held`'s head table. `None` for no descriptor, an entry that isn't a
/// `HeadSpec` (an odd frame, an empty head), or a `held` too short for the
/// table and its heads' samples; empty when held whole.
pub fn head_table(held: &[u8]) -> Option<HeadTable<'_>> {
    head_table_of(held, Descriptor::from_bytes(held)?)
}

/// `head_table` under `d`, `held`'s descriptor already read.
pub fn head_table_of(held: &[u8], d: Descriptor) -> Option<HeadTable<'_>> {
    let entries = held
        .get(DESC_LEN..)?
        .as_chunks()
        .0
        .get(..usize::from(d.heads))?;
    // In `u64`: past any frames a table can name.
    let mut need = (DESC_LEN + HEAD_ENTRY * entries.len()) as u64;
    for e in entries {
        need += u64::from(HeadSpec::from_bytes(e)?.bytes().get());
    }
    (held.len() as u64 >= need).then_some(HeadTable { entries })
}
