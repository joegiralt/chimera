//! Card storage vocabulary.

use core::ops::ControlFlow;

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
    /// The MBR type byte, for the log only.
    NotFat(u8),
    /// A FAT volume whose boot sector fails validation.
    BadBootSector,
}

/// Bytes per `ReadSink::chunk`, one SD block.
pub const CHUNK: usize = 512;

/// The three directories Chimera keeps on the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dir {
    /// `/CHIMERA`
    Chimera,
    /// `/CHIMERA/PROJECTS`
    Projects,
    /// `/CHIMERA/SOUNDS`
    Sounds,
}

/// An 8.3 name in one of the `Dir`s: A-Z and 0-9 only, so it is valid on any
/// FAT without long-name entries.
///
/// ```compile_fail,E0451
/// use chimera_hal::store::{Dir, FileName};
/// let _ = FileName { dir: Dir::Chimera, stem: [0; 8], ext: [0; 3], stem_len: 1, ext_len: 0 };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileName {
    dir: Dir,
    stem: [u8; 8],
    ext: [u8; 3],
    stem_len: u8,
    ext_len: u8,
}

impl FileName {
    /// `stem` is 1..=8 and `ext` 0..=3 bytes of A-Z 0-9.
    pub fn new(dir: Dir, stem: &[u8], ext: &[u8]) -> Option<FileName> {
        let ok = |b: &[u8], min: usize, max: usize| {
            (min..=max).contains(&b.len())
                && b.iter()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        };
        if !ok(stem, 1, 8) || !ok(ext, 0, 3) {
            return None;
        }
        let mut f = FileName {
            dir,
            stem: [0; 8],
            ext: [0; 3],
            stem_len: stem.len() as u8,
            ext_len: ext.len() as u8,
        };
        f.stem[..stem.len()].copy_from_slice(stem);
        f.ext[..ext.len()].copy_from_slice(ext);
        Some(f)
    }

    pub fn dir(&self) -> Dir {
        self.dir
    }

    pub fn stem(&self) -> &[u8] {
        &self.stem[..self.stem_len as usize]
    }

    pub fn ext(&self) -> &[u8] {
        &self.ext[..self.ext_len as usize]
    }
}

/// Why a store operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    NoCard,
    Unsupported(Unsupported),
    NotFound,
    Full,
    Timeout,
    /// The card was swapped since `mount`; carries the id now in the slot.
    VolumeChanged(VolumeId),
    /// The file's chain or length is inconsistent: a file error, not a card fault.
    Corrupt,
    Io,
}

impl StoreError {
    /// The line the UI shows.
    pub fn message(self) -> &'static str {
        match self {
            StoreError::NoCard => "NO CARD",
            StoreError::Unsupported(Unsupported::Exfat) => "CARD IS EXFAT: FORMAT FAT32",
            StoreError::Unsupported(Unsupported::NoPartitionTable) => "CARD HAS NO PARTITION TABLE",
            StoreError::Unsupported(Unsupported::NotFat(_)) => "CARD IS NOT FAT16/FAT32",
            StoreError::Unsupported(Unsupported::BadBootSector) => "CARD FORMAT IS DAMAGED",
            StoreError::NotFound => "FILE NOT FOUND",
            StoreError::Full => "CARD FULL",
            StoreError::Timeout => "CARD TIMEOUT",
            StoreError::VolumeChanged(_) => "CARD CHANGED",
            StoreError::Corrupt => "FILE IS DAMAGED",
            StoreError::Io => "CARD ERROR",
        }
    }
}

/// Where a `Store::write` body puts its bytes.
pub trait ByteSink {
    fn put(&mut self, bytes: &[u8]) -> Result<(), StoreError>;
}

/// Receives a file from `Store::read`; `Break` stops the read early and is not an error.
pub trait ReadSink {
    fn begin(&mut self, len: u32) -> ControlFlow<()>;
    /// At most `CHUNK` bytes.
    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()>;
}

pub trait Store {
    /// (Re)init the medium if needed, open the volume, read its id, close it.
    fn mount(&mut self) -> Result<VolumeId, StoreError>;
    /// Each op below: open volume, check id == vol (else `VolumeChanged`,
    /// nothing touched), act, close every handle.
    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError>;
    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError>;
    /// Replaces the file; returns the bytes written. What `body` put before an
    /// `Err` stays. A `file.dir()` that was never made is `NotFound`.
    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError>;
    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError>;
    /// An existing directory is `Ok`. Making `Projects` or `Sounds` before `Chimera`
    /// exists is `NotFound`.
    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError>;
}
