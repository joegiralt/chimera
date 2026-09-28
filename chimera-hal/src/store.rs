//! Card storage vocabulary.

/// A mounted volume's identity: a swap changes one or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeId {
    pub serial: u32,
    pub label: [u8; 11],
}

/// A card Chimera can't mount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsupported {
    Exfat,
    NoPartitionTable,
    /// The first partition's type byte.
    NotFat(u8),
}
