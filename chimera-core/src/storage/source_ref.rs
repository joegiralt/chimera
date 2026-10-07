//! A source's ref (ADR 0087): what a project and a Sound store of a TAPE
//! source, never its audio. A storage type: `project` names it too.

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
