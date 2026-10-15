//! A source's descriptor (spec § 1 What the arena holds): the 16 B at the
//! start of `held`. Bank 1's with `bank2` (storage and the pool read it),
//! as `read` re-exports it; `read`'s decode is bank 2's.

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
/// 0 when held whole), then 8 reserved zero bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Descriptor {
    pub shape: Shape,
    pub count: u8,
    pub rate: u16,
    pub head_frames: u32,
}

impl Descriptor {
    pub fn to_bytes(&self) -> [u8; DESC_LEN] {
        let mut b = [0; DESC_LEN];
        (b[0], b[1]) = (self.shape as u8, self.count);
        b[2..4].copy_from_slice(&self.rate.to_le_bytes());
        b[4..8].copy_from_slice(&self.head_frames.to_le_bytes());
        b
    }

    /// `None` for a short slice, an unknown shape, a count outside its
    /// shape's `1..=max` (a `Plain` holds exactly 1), a rate outside
    /// 4 000..=48 000 (the pool spec's range), `head_frames` set on anything but a `ZoneSet`, or a set
    /// reserved byte.
    pub fn from_bytes(b: &[u8]) -> Option<Descriptor> {
        let b: &[u8; DESC_LEN] = b.get(..DESC_LEN)?.try_into().ok()?;
        let shape = Shape::from_code(b[0])?;
        let d = Descriptor {
            shape,
            count: b[1],
            rate: u16::from_le_bytes([b[2], b[3]]),
            head_frames: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
        };
        let ok = (1..=shape.max_count()).contains(&d.count)
            && (4_000..=48_000).contains(&d.rate)
            && (d.head_frames == 0 || shape == Shape::ZoneSet)
            && b[8..].iter().all(|&x| x == 0);
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
