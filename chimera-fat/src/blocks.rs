//! Volume-relative 512 B block I/O, and the errors the FAT core returns.

use chimera_hal::store::StoreError;

pub const BLOCK: usize = 512;

/// A volume's blocks, numbered from its boot sector.
pub trait Blocks {
    type Error;
    fn read(&mut self, lba: u32, buf: &mut [u8; BLOCK]) -> Result<(), Self::Error>;
    fn write(&mut self, lba: u32, buf: &[u8; BLOCK]) -> Result<(), Self::Error>;
}

/// Why a FAT core operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsError<E> {
    /// The block device failed.
    Dev(E),
    NotFound,
    Full,
    /// A chain, entry or directory the volume can't hold.
    Corrupt,
    /// A write body's own error.
    Body(StoreError),
}
