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
    /// FAT32 with mirroring off (BPB_ExtFlags bit 7): one active FAT that
    /// the others don't follow. Chimera keeps every FAT in step.
    FatNotMirrored,
}

/// Bytes per `ReadSink::chunk`, one SD block.
pub const CHUNK: usize = 512;

/// The directories Chimera keeps on the card.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dir {
    /// `/CHIMERA`
    Chimera,
    /// `/CHIMERA/PROJECTS`
    Projects,
    /// `/CHIMERA/SOUNDS`
    Sounds,
    /// `/CHIMERA/BENCH`: the bench build's card test file.
    Bench,
    /// `/CHIMERA/TAPES`: TAPE's sources, write-once (ADR 0087).
    Tapes,
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
    /// No free cluster for the file: a file-level condition, not a card
    /// fault. The card stays usable (a delete makes room) and nothing
    /// re-inits.
    Full,
    Timeout,
    /// The card was swapped since `mount`; carries the id now in the slot.
    VolumeChanged(VolumeId),
    /// The file's chain or length is inconsistent: a file error, not a card fault.
    Corrupt,
    /// The file is marked read-only on the card: nothing rewrites or
    /// deletes it. A file error, not a card fault.
    ReadOnly,
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
            StoreError::Unsupported(Unsupported::FatNotMirrored) => {
                "CARD FAT NOT MIRRORED: FORMAT IT"
            }
            StoreError::NotFound => "FILE NOT FOUND",
            StoreError::Full => "CARD FULL",
            StoreError::Timeout => "CARD TIMEOUT",
            StoreError::VolumeChanged(_) => "CARD CHANGED",
            StoreError::Corrupt => "FILE IS DAMAGED",
            StoreError::ReadOnly => "FILE IS READ-ONLY",
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
    /// `read` from byte `offset` (a card-row load's 4 KiB a pass): `begin`
    /// gets the bytes from there on. A store that can't seek says `Io`, so
    /// a load fails loudly and never plays the wrong audio.
    fn read_at(
        &mut self,
        _vol: VolumeId,
        _file: FileName,
        _offset: u32,
        _sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        Err(StoreError::Io)
    }
    /// Replaces the file; returns the bytes written. What `body` put before an
    /// `Err` stays: not atomic, so callers needing atomicity write the side
    /// nobody reads. A `file.dir()` that was never made is `NotFound`.
    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError>;
    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError>;
    /// An existing directory is `Ok`. Making any other `Dir` before `Chimera`
    /// exists is `NotFound`.
    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError>;
    /// Marks `file` read-only: from then on `write` and `delete` refuse it
    /// (`ReadOnly`). One already sealed is `Ok`, with nothing written. A
    /// store that can't seal says `Io`, as does every store for a file
    /// outside `Dir::Tapes`: only a source is write-once.
    fn seal(&mut self, _vol: VolumeId, _file: FileName) -> Result<(), StoreError> {
        Err(StoreError::Io)
    }
    /// The lost-cluster sweep (#360), one operation at boot: clusters the
    /// FAT holds that no directory on the volume reaches. `marks` lends
    /// the reachability bitmap, one bit a cluster; the sweep zeroes it.
    /// Any doubt is an `Err` before the first write, with the stats so far.
    /// A store with no FAT sweeps nothing.
    fn sweep(
        &mut self,
        _vol: VolumeId,
        _mode: Sweep,
        _marks: &mut [u8],
    ) -> Result<Swept, (Doubt, SweepStats)> {
        Ok(Swept::default())
    }
}

/// What a sweep does with the clusters it finds lost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sweep {
    /// Counts them; never writes.
    DryRun,
    /// Frees them.
    Free,
}

/// The volume a sweep ran on, and its phases' times.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SweepStats {
    /// Blocks per cluster.
    pub spc: u32,
    pub clusters: u32,
    /// Bytes in one FAT.
    pub fat_bytes: u32,
    pub compare_ms: u32,
    pub mark_ms: u32,
    pub count_ms: u32,
}

/// A sweep that ran to its end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Swept {
    pub lost: u32,
    /// 0 in `DryRun`.
    pub freed: u32,
    /// FAT 2 had one sector behind FAT 1.
    pub stale: bool,
    /// That sector was copied from FAT 1: never in `DryRun`.
    pub healed: bool,
    pub stats: SweepStats,
}

impl Swept {
    /// Nothing left for a later sweep: no sector behind (what only FAT 1
    /// held there waits for the boot after its copy), and every lost
    /// cluster freed, so none in `DryRun`.
    pub fn settled(&self) -> bool {
        !self.stale && self.freed == self.lost
    }
}

/// Why a sweep stopped before writing anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Doubt {
    /// FAT 2 differs from FAT 1 in more than one sector, or the one
    /// differing sector sends a reached cluster two ways.
    FatsDiffer,
    /// A directory block or a FAT sector didn't read.
    Unreadable,
    /// A directory's chain or entries are malformed.
    DirBroken,
    /// A file's chain breaks, or is shorter than its length.
    FileShort,
    /// A cluster reached twice.
    CrossLinked,
    /// Deeper than `MAX_DEPTH`, or more than `MAX_DIRS` directories.
    TooDeep,
    /// More clusters than the bitmap's bits.
    NoRoom,
    /// The card timed out, left, or was swapped.
    Card(StoreError),
}

impl Doubt {
    /// The word `status` prints.
    pub fn word(self) -> &'static str {
        match self {
            Doubt::FatsDiffer => "fats-differ",
            Doubt::Unreadable => "unreadable",
            Doubt::DirBroken => "dir-broken",
            Doubt::FileShort => "file-short",
            Doubt::CrossLinked => "cross-linked",
            Doubt::TooDeep => "too-deep",
            Doubt::NoRoom => "no-room",
            Doubt::Card(StoreError::NoCard) => "no-card",
            Doubt::Card(StoreError::Timeout) => "timeout",
            Doubt::Card(StoreError::VolumeChanged(_)) => "card-changed",
            Doubt::Card(StoreError::Unsupported(_)) => "unsupported",
            // The sweep's read-only phases asked to write: a bug.
            Doubt::Card(StoreError::ReadOnly) => "write-refused",
            Doubt::Card(_) => "card-error",
        }
    }
}

/// What a boot's sweep ended with.
pub type SweepSeen = Result<Swept, (Doubt, SweepStats)>;

/// A `Store` whose operations' time can be capped: the console's `put`
/// holds each to what is left of its own deadline.
pub trait Files: Store {
    /// Each operation armed from now on ends after `ms` in all; `None`: the
    /// medium's own cap.
    fn cap(&mut self, ms: Option<u32>);
}
