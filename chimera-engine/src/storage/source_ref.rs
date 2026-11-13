//! A source's ref (ADR 0087): what a project and a Sound store of a TAPE
//! source, never its audio, and a Sound's tuning of a zone set. A storage
//! type: `project` names it too.

use core::num::NonZeroU8;

use crate::dsp::tape_engine::read::MAX_ZONES;

/// A `.SRC`'s stem: its content's CRC, which names the file
/// (`source_name`). Not a size, though both are `u32` on the card.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stem(u32);

impl Stem {
    #[optimize(size)]
    pub const fn new(crc: u32) -> Stem {
        Stem(crc)
    }

    #[optimize(size)]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// A source's identity: its card file's CRC, its length and its rate. A
/// Sound carries one, never audio. 10 B on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceRef {
    pub crc: u32,
    pub frames: u32,
    pub rate: u16,
}

impl SourceRef {
    pub const LEN: usize = 10;

    /// Its file's stem.
    #[optimize(size)]
    pub const fn stem(self) -> Stem {
        Stem(self.crc)
    }

    #[optimize(size)]
    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut b = [0; Self::LEN];
        b[..4].copy_from_slice(&self.crc.to_le_bytes());
        b[4..8].copy_from_slice(&self.frames.to_le_bytes());
        b[8..].copy_from_slice(&self.rate.to_le_bytes());
        b
    }

    /// `None` unless exactly `LEN` bytes.
    #[optimize(size)]
    pub fn from_bytes(b: &[u8]) -> Option<SourceRef> {
        let b: &[u8; Self::LEN] = b.try_into().ok()?;
        let (crc, rest) = b.split_first_chunk::<4>()?;
        let (frames, rate) = rest.split_first_chunk::<4>()?;
        Some(SourceRef {
            crc: u32::from_le_bytes(*crc),
            frames: u32::from_le_bytes(*frames),
            rate: u16::from_le_bytes(rate.try_into().ok()?),
        })
    }
}

/// A zone set's tuning as a Sound holds it (spec § 8): each zone's `root`
/// (MIDI cents) and `period` (Q16.16, 0 unpitched), in zone order, copied
/// from the file's zone labels when SOURCE picks the set. The voice tunes
/// from here, never from the file, so a project tunes the same on every
/// card. 1 to `MAX_ZONES` zones. Kept as its record's 6 B a zone, so every
/// Sound in AXI pays 145 B for it, not 200. Only `new` makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoneTune {
    /// `root` then `period`, LE; past `n`, zero.
    zones: [[u8; ZoneTune::ZONE_LEN]; MAX_ZONES as usize],
    /// 1 to `MAX_ZONES`: `Option<ZoneTune>` takes no more room.
    n: NonZeroU8,
}

const _: () = assert!(size_of::<Option<ZoneTune>>() == 145);

impl ZoneTune {
    /// One zone's bytes: `root`, then `period`, LE.
    pub const ZONE_LEN: usize = 6;

    #[optimize(size)]
    pub fn new(zones: &[(u16, u32)]) -> Option<ZoneTune> {
        let n = u8::try_from(zones.len()).ok().and_then(NonZeroU8::new)?;
        if n.get() > MAX_ZONES {
            return None;
        }
        let mut all = [[0; Self::ZONE_LEN]; MAX_ZONES as usize];
        for (b, (root, period)) in all.iter_mut().zip(zones) {
            b[..2].copy_from_slice(&root.to_le_bytes());
            b[2..].copy_from_slice(&period.to_le_bytes());
        }
        Some(ZoneTune { zones: all, n })
    }

    /// Each zone's `(root, period)`, in zone order.
    #[optimize(size)]
    pub fn zones(&self) -> impl ExactSizeIterator<Item = (u16, u32)> + '_ {
        (0..self.n.get() as usize).map(|i| self.zone(i))
    }

    /// Zone `i`'s `(root, period)`. The tune isn't typed against its set's
    /// zone count (the ref doesn't count zones), so a zone past it plays
    /// at the last tune's pitch; tunes past the set's zones go unused.
    #[optimize(size)]
    pub fn zone(&self, i: usize) -> (u16, u32) {
        let b = self.zones[i.min(self.n.get() as usize - 1)];
        (
            u16::from_le_bytes([b[0], b[1]]),
            u32::from_le_bytes([b[2], b[3], b[4], b[5]]),
        )
    }

    /// The record's payload, `ZONE_LEN` bytes a zone: as it is held, so
    /// writing it builds nothing on the stack.
    #[optimize(size)]
    pub fn record(&self) -> &[u8] {
        self.zones[..self.n.get() as usize].as_flattened()
    }

    /// `None` unless 1 to `MAX_ZONES` whole zones.
    #[optimize(size)]
    pub fn from_record(p: &[u8]) -> Option<ZoneTune> {
        let (zones, rest) = p.as_chunks::<{ Self::ZONE_LEN }>();
        let n = u8::try_from(zones.len()).ok().and_then(NonZeroU8::new)?;
        if !rest.is_empty() || n.get() > MAX_ZONES {
            return None;
        }
        let mut all = [[0; Self::ZONE_LEN]; MAX_ZONES as usize];
        all[..zones.len()].copy_from_slice(zones);
        Some(ZoneTune { zones: all, n })
    }
}

/// A Sound's TAPE source: its ref, and a zone set's tuning with it. A
/// `ZoneTune` exists only beside the `SourceRef` it tunes. The ref doesn't
/// count zones, so a tune's zone count can't be typed against the set's:
/// TAPE Task 13's voice clamps a mismatch (zones past the tune play at the
/// last tune's pitch, tunes past the zones are unused).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TapeSource {
    pub r: SourceRef,
    pub tune: Option<ZoneTune>,
}

impl From<SourceRef> for TapeSource {
    /// A source with no zone tuning: a `Plain` or `Transwave`.
    #[optimize(size)]
    fn from(r: SourceRef) -> Self {
        TapeSource { r, tune: None }
    }
}
