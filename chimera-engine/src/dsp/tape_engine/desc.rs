//! A source's descriptor (spec § 1 What the arena holds): the 16 B at the
//! start of `held`. Bank 1's with `bank2` (storage and the pool read it),
//! as `read` re-exports it; `read`'s decode is bank 2's.

use core::num::NonZeroU8;

use chimera_hal::units::Bytes;

use crate::stream::units::{Frames, Packing, SourceRate};

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
    const fn max_count(self) -> u8 {
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

/// Whether a `Plain`'s samples are all in `held`, or streamed past its
/// heads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Held {
    Whole,
    /// Its heads (slice 1 § 5.1), and its reach past each note (`HEAD_MS`
    /// at its rate); each head's own frames are its table entry's.
    Streamed {
        heads: NonZeroU8,
        reach: Frames,
    },
}

/// A descriptor's shape with its count. A `ZoneSet`'s heads are its
/// zones'; its `head_frames` bytes are kept as read (`reach`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Plain(Held),
    Transwave { count: NonZeroU8 },
    ZoneSet { count: NonZeroU8, reach: Frames },
}

/// `held`'s first 16 B: `shape`, `count`, `rate` (LE), `head_frames` (LE,
/// 0 when held whole), `heads`, then 7 reserved zero bytes. Only `new`
/// builds one, so every one `to_bytes` writes `from_bytes` reads back.
///
/// ```compile_fail,E0451
/// use chimera_engine::dsp::tape_engine::read::{Descriptor, Shape};
/// let d = Descriptor::whole(Shape::Plain, 1, 48_000).unwrap();
/// let _ = Descriptor { ..d };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    rate: SourceRate,
    layout: Layout,
}

impl Descriptor {
    /// `None` for a count outside its shape's `1..=max` (a `Plain` holds
    /// exactly 1), a rate outside `SourceRate`'s, `reach` on a
    /// `Transwave`, a `Plain`'s `heads` and `reach` not set together, or
    /// `heads` on a `ZoneSet`.
    pub const fn new(
        shape: Shape,
        count: u8,
        rate: u16,
        reach: Frames,
        heads: u8,
    ) -> Option<Descriptor> {
        let r = reach.get();
        // One test, as the bytes are read: each early return would be a
        // branch a caller's inliner weighs.
        let held_ok = match shape {
            Shape::Plain => (heads == 0) == (r == 0),
            Shape::Transwave => heads == 0 && r == 0,
            Shape::ZoneSet => heads == 0,
        };
        let (Some(rate), Some(n), true) = (
            SourceRate::new(rate),
            NonZeroU8::new(count),
            held_ok && count <= shape.max_count(),
        ) else {
            return None;
        };
        let layout = match (shape, NonZeroU8::new(heads)) {
            (Shape::Plain, None) => Layout::Plain(Held::Whole),
            (Shape::Plain, Some(heads)) => Layout::Plain(Held::Streamed { heads, reach }),
            (Shape::Transwave, _) => Layout::Transwave { count: n },
            (Shape::ZoneSet, _) => Layout::ZoneSet { count: n, reach },
        };
        Some(Descriptor { rate, layout })
    }

    /// A source held whole: `new` with no heads and no reach.
    pub const fn whole(shape: Shape, count: u8, rate: u16) -> Option<Descriptor> {
        Self::new(shape, count, rate, Frames::new(0), 0)
    }

    pub const fn rate(&self) -> SourceRate {
        self.rate
    }

    pub const fn layout(&self) -> Layout {
        self.layout
    }

    pub const fn shape(&self) -> Shape {
        match self.layout {
            Layout::Plain(_) => Shape::Plain,
            Layout::Transwave { .. } => Shape::Transwave,
            Layout::ZoneSet { .. } => Shape::ZoneSet,
        }
    }

    pub const fn count(&self) -> u8 {
        match self.layout {
            Layout::Plain(_) => 1,
            Layout::Transwave { count } | Layout::ZoneSet { count, .. } => count.get(),
        }
    }

    /// A `Plain`'s; every other shape is held whole.
    pub const fn held(&self) -> Held {
        match self.layout {
            Layout::Plain(h) => h,
            _ => Held::Whole,
        }
    }

    /// Its samples stream past its heads: not all in `held`.
    pub const fn is_streamed(&self) -> bool {
        matches!(self.held(), Held::Streamed { .. })
    }

    /// Its head table's entries: 0 when held whole.
    pub const fn heads(&self) -> u8 {
        match self.held() {
            Held::Streamed { heads, .. } => heads.get(),
            Held::Whole => 0,
        }
    }

    /// The `head_frames` bytes: a streamed `Plain`'s reach past each note,
    /// 0 on any other but a `ZoneSet` (kept as read).
    pub const fn reach(&self) -> Frames {
        match self.layout {
            Layout::Plain(Held::Streamed { reach, .. }) | Layout::ZoneSet { reach, .. } => reach,
            _ => Frames::new(0),
        }
    }

    pub fn to_bytes(&self) -> [u8; DESC_LEN] {
        let mut b = [0; DESC_LEN];
        (b[0], b[1]) = (self.shape() as u8, self.count());
        b[2..4].copy_from_slice(&self.rate.get().to_le_bytes());
        b[4..8].copy_from_slice(&self.reach().get().to_le_bytes());
        b[8] = self.heads();
        b
    }

    /// `None` for a short slice, an unknown shape, a descriptor `new`
    /// refuses, or a set reserved byte.
    pub fn from_bytes(b: &[u8]) -> Option<Descriptor> {
        let b: &[u8; DESC_LEN] = b.get(..DESC_LEN)?.try_into().ok()?;
        let shape = Shape::from_code(b[0])?;
        let reach = Frames::new(u32::from_le_bytes([b[4], b[5], b[6], b[7]]));
        let rate = u16::from_le_bytes([b[2], b[3]]);
        let reserved = b[9..].iter().all(|&x| x == 0);
        Descriptor::new(shape, b[1], rate, reach, b[8]).filter(|_| reserved)
    }

    /// Where the samples start in `held`: after a `ZoneSet`'s layout.
    pub(super) fn data_start(&self) -> usize {
        match self.layout {
            Layout::ZoneSet { count, .. } => DESC_LEN + ZONE_LAYOUT_LEN * count.get() as usize,
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

/// Writes a streamed `Plain`'s descriptor at `rate`, reaching `reach`,
/// and its table of `heads` into `out`; where the first head's samples
/// go. `None` if `out` is shorter, or no descriptor holds them (none, or
/// past 255, or no reach).
pub fn write_head_table(
    rate: SourceRate,
    reach: Frames,
    heads: &[HeadSpec],
    out: &mut [u8],
) -> Option<usize> {
    let n = u8::try_from(heads.len()).ok().and_then(NonZeroU8::new);
    let (Some(heads_n), true) = (n, reach.get() > 0) else {
        return None;
    };
    let held = Held::Streamed {
        heads: heads_n,
        reach,
    };
    let d = Descriptor {
        rate,
        layout: Layout::Plain(held),
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
        .get(..usize::from(d.heads()))?;
    // In `u64`: past any frames a table can name.
    let mut need = (DESC_LEN + HEAD_ENTRY * entries.len()) as u64;
    for e in entries {
        need += u64::from(HeadSpec::from_bytes(e)?.bytes().get());
    }
    (held.len() as u64 >= need).then_some(HeadTable { entries })
}
