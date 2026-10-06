//! A source's ref (ADR 0087): what a project and a Sound store of a TAPE
//! source, never its audio, and a Sound's tuning of a zone set. A storage
//! type: `project` names it too.

use crate::dsp::tape_engine::read::MAX_ZONES;

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

    pub fn to_bytes(&self) -> [u8; Self::LEN] {
        let mut b = [0; Self::LEN];
        b[..4].copy_from_slice(&self.crc.to_le_bytes());
        b[4..8].copy_from_slice(&self.frames.to_le_bytes());
        b[8..].copy_from_slice(&self.rate.to_le_bytes());
        b
    }

    /// `None` unless exactly `LEN` bytes.
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
/// card. 1 to `MAX_ZONES` zones; 6 B a zone on disk. Only `new` makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoneTune {
    /// Past `n`, zero.
    zones: [(u16, u32); MAX_ZONES as usize],
    n: u8,
}

impl ZoneTune {
    /// One zone's bytes: `root`, then `period`, LE.
    pub const ZONE_LEN: usize = 6;

    pub fn new(zones: &[(u16, u32)]) -> Option<ZoneTune> {
        let n = u8::try_from(zones.len()).ok()?;
        if !(1..=MAX_ZONES).contains(&n) {
            return None;
        }
        let mut all = [(0, 0); MAX_ZONES as usize];
        all[..zones.len()].copy_from_slice(zones);
        Some(ZoneTune { zones: all, n })
    }

    pub fn zones(&self) -> &[(u16, u32)] {
        &self.zones[..self.n as usize]
    }

    /// The record's payload: `ZONE_LEN` bytes a zone.
    pub fn to_record(&self) -> super::RecordBuf {
        let mut r = super::RecordBuf::new();
        for (root, period) in self.zones() {
            r.bytes(&root.to_le_bytes());
            r.u32(*period);
        }
        r
    }

    /// `None` unless whole zones that `new` takes.
    pub fn from_record(p: &[u8]) -> Option<ZoneTune> {
        let (zones, rest) = p.as_chunks::<{ Self::ZONE_LEN }>();
        if !rest.is_empty() || zones.len() > MAX_ZONES as usize {
            return None;
        }
        let mut all = [(0, 0); MAX_ZONES as usize];
        for (t, b) in all.iter_mut().zip(zones) {
            *t = (
                u16::from_le_bytes([b[0], b[1]]),
                u32::from_le_bytes([b[2], b[3], b[4], b[5]]),
            );
        }
        ZoneTune::new(&all[..zones.len()])
    }
}
