//! Card storage vocabulary.

use core::ops::ControlFlow;

use crate::card_bus::CardFault;
use crate::units::{BlockCount, Bytes, FileBlock, Lba};

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
    /// `/CHIMERA/SYSTEM`: bank 2's staged image (ADR 0137). The stock
    /// bootloader reads only `.bin` files in the root.
    System,
    /// The card's root: listed and read, for a dropped bank-2 image (ADR
    /// 0137). Always there; never made or removed; `write` and `delete`
    /// refuse it (`ReadOnly`), `seal` too (`Io`).
    Root,
}

/// An 8.3 name in one of the `Dir`s: A-Z and 0-9 only, so it is valid on any
/// FAT without long-name entries. A root stem may also hold `~`: the short
/// name a host gives a long one (`CHIMER~1.BK2`).
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
    /// `stem` is 1..=8 and `ext` 0..=3 bytes of A-Z 0-9. Const, so a fixed
    /// name is checked at compile time.
    pub const fn new(dir: Dir, stem: &[u8], ext: &[u8]) -> Option<FileName> {
        let tilde = matches!(dir, Dir::Root);
        if !name_ok(stem, 1, 8, tilde) || !name_ok(ext, 0, 3, false) {
            return None;
        }
        let mut f = FileName {
            dir,
            stem: [0; 8],
            ext: [0; 3],
            stem_len: stem.len() as u8,
            ext_len: ext.len() as u8,
        };
        f.stem.split_at_mut(stem.len()).0.copy_from_slice(stem);
        f.ext.split_at_mut(ext.len()).0.copy_from_slice(ext);
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

/// `min..=max` bytes of A-Z 0-9, and `~` if `tilde`.
const fn name_ok(b: &[u8], min: usize, max: usize, tilde: bool) -> bool {
    if b.len() < min || b.len() > max {
        return false;
    }
    let mut i = 0;
    while i < b.len() {
        if !(b[i].is_ascii_uppercase() || b[i].is_ascii_digit() || (tilde && b[i] == b'~')) {
            return false;
        }
        i += 1;
    }
    true
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
    /// The streams held the card bus past the UI's wait: nothing was sent,
    /// and the card is as it was. Not a card fault; nothing re-inits.
    Busy,
    /// A delete or replace of a source the streams read (a registry row):
    /// refused, nothing written. A file error, not a card fault.
    Streaming,
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
            StoreError::Busy => "CARD BUSY",
            StoreError::Streaming => "SOURCE IS STREAMING",
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
    /// The streams' card reads faulted: `recover` is owed. A store no
    /// stream reads never faults.
    fn read_fault(&self) -> CardFault {
        CardFault::Clear
    }
    /// After a read fault: the medium re-inited, then `mount`, whose
    /// success clears the fault. `Busy`: the card untouched.
    fn recover(&mut self) -> Result<VolumeId, StoreError> {
        self.mount()
    }
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
    /// `file`'s length and where its blocks lie on the device, for raw
    /// reads that bypass the store. A store with no device blocks says
    /// `Io`; a file in more than `MAX_RUNS` runs is `Fragmented`.
    fn runs(&mut self, _vol: VolumeId, _file: FileName) -> Result<FileRuns, RunsError> {
        Err(RunsError::Store(StoreError::Io))
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
    /// The sweep mark (ADR 0094): whether a change since the last settled
    /// sweep may have stranded clusters. A store with no FAT has none to
    /// strand: `Unused`.
    fn mark(&mut self, _vol: VolumeId) -> Result<Mark, StoreError> {
        Ok(Mark::Unused)
    }
    /// After a sweep that `settled`, the mark to `Clean`, made if it is
    /// `Missing`; `true` once it is. Any other sweep keeps the debt, a
    /// doubt count reset to plain `Dirty`: `false`.
    fn settle(&mut self, _vol: VolumeId, _swept: &Swept) -> Result<bool, StoreError> {
        Ok(false)
    }
    /// After a boot's sweep that doubted. With `count`, one more doubting
    /// boot on the mark, in place (a card in doubt allocates nothing);
    /// without (an asked sweep), the count the mark carried put back. Returns the count now,
    /// 0 where there is no mark to keep it.
    fn note_doubt(&mut self, _vol: VolumeId, _count: bool) -> Result<u8, StoreError> {
        Ok(0)
    }
    /// Asks the next boot to sweep: the mark to `Asked`, made if it is
    /// `Missing`. Returns the mark now: `Unused` (no `/CHIMERA`, nothing
    /// to sweep) or `Dirty` (a mark it can't rewrite, owing already) are
    /// left as they are.
    fn request_sweep(&mut self, _vol: VolumeId) -> Result<Mark, StoreError> {
        Ok(Mark::Unused)
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

/// Runs `Extents` keeps per file.
pub const MAX_RUNS: usize = 32;

/// A file in more than `MAX_RUNS` runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fragmented {
    pub runs: u32,
}

/// Collects a file's runs, in order; `finish` is the only way to an
/// `Extents`.
pub struct Runs {
    runs: [(Lba, BlockCount); MAX_RUNS],
    seen: u32,
}

impl Default for Runs {
    fn default() -> Self {
        Self::new()
    }
}

impl Runs {
    pub const fn new() -> Self {
        Runs {
            runs: [(Lba::new(0), BlockCount::ONE); MAX_RUNS],
            seen: 0,
        }
    }

    /// The next run: its first device block and its length. Past
    /// `MAX_RUNS` it is counted and not kept.
    pub fn push(&mut self, first: Lba, blocks: BlockCount) {
        if let Some(slot) = self.runs.get_mut(self.seen as usize) {
            *slot = (first, blocks);
        }
        self.seen = self.seen.saturating_add(1);
    }

    pub fn finish(self) -> Result<Extents, Fragmented> {
        if self.seen as usize > MAX_RUNS {
            return Err(Fragmented { runs: self.seen });
        }
        Ok(Extents {
            runs: self.runs,
            kept: self.seen as usize,
        })
    }
}

/// A file's runs on the device: at most `MAX_RUNS`, whole.
#[derive(Clone, Copy, Debug)]
pub struct Extents {
    runs: [(Lba, BlockCount); MAX_RUNS],
    kept: usize,
}

impl Extents {
    pub fn runs(&self) -> u32 {
        self.kept as u32
    }

    /// The first run's first device block; block 0 for an empty file,
    /// which has no run.
    pub fn first(&self) -> Lba {
        self.runs[0].0
    }

    fn kept(&self) -> &[(Lba, BlockCount)] {
        self.runs.get(..self.kept).unwrap_or_default()
    }

    /// File block `at` on the device, and the blocks left in its run.
    pub fn locate(&self, at: FileBlock) -> Option<(Lba, BlockCount)> {
        let mut at = at.get();
        for &(first, n) in self.kept() {
            if at < n.get() {
                return Some((first.offset(at)?, BlockCount::new(n.get() - at)?));
            }
            at -= n.get();
        }
        None
    }

    /// The reads of file blocks `from..from + len`, each at most `max`
    /// blocks and none across a run's end: (device block, blocks). Stops
    /// short past the last run.
    pub fn transfers(&self, from: FileBlock, len: u32, max: BlockCount) -> Transfers<'_> {
        Transfers {
            ext: self,
            at: from.get(),
            end: from.get().saturating_add(len),
            max,
        }
    }
}

pub struct Transfers<'a> {
    ext: &'a Extents,
    at: u32,
    end: u32,
    max: BlockCount,
}

impl Iterator for Transfers<'_> {
    type Item = (Lba, BlockCount);

    fn next(&mut self) -> Option<(Lba, BlockCount)> {
        if self.at >= self.end {
            return None;
        }
        let (lba, left) = self.ext.locate(FileBlock::new(self.at))?;
        let n = left.get().min(self.max.get()).min(self.end - self.at);
        self.at += n;
        Some((lba, BlockCount::new(n)?))
    }
}

/// A file's length, where it lies on the device, and its seal.
#[derive(Clone, Copy, Debug)]
pub struct FileRuns {
    pub ext: Extents,
    pub len: Bytes,
    pub seal: Seal,
}

/// Whether a file is sealed read-only (`Store::seal`): a source is sealed
/// only once it read back whole and its samples hashed to its CRC, so an
/// open one may be a cut write's remains, unverified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seal {
    Sealed,
    Open,
}

/// Why `Store::runs` gave none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunsError {
    Store(StoreError),
    Fragmented(Fragmented),
}

impl From<StoreError> for RunsError {
    fn from(e: StoreError) -> Self {
        RunsError::Store(e)
    }
}

/// Blocks between synthetic files' first blocks: 32 MiB.
pub const SYNTHETIC_STRIDE: u32 = 1 << 16;
// The 16-bit fold times the stride fits a block number.
const _: () = assert!((u16::MAX as u64 + 1) * SYNTHETIC_STRIDE as u64 <= 1 << 32);

/// Where a store with no device puts `file`: its stem's 16-bit fold times
/// `SYNTHETIC_STRIDE`, so a file under 32 MiB overlaps no other's runs
/// (two stems may fold alike).
pub fn synthetic_lba(file: FileName) -> Lba {
    let fold = file
        .stem()
        .iter()
        .fold(0u16, |h, &b| h.wrapping_mul(31).wrapping_add(u16::from(b)));
    Lba::new(u32::from(fold) * SYNTHETIC_STRIDE)
}

/// `Store::runs` for a store with no device: one run at `synthetic_lba`.
/// A file over the stride is `Io`.
pub fn synthetic_runs(file: FileName, len: Bytes, seal: Seal) -> Result<FileRuns, RunsError> {
    let blocks = len.get().div_ceil(CHUNK as u32);
    let mut r = Runs::new();
    if let Some(n) = BlockCount::new(blocks) {
        if blocks > SYNTHETIC_STRIDE {
            return Err(StoreError::Io.into());
        }
        r.push(synthetic_lba(file), n);
    }
    let ext = r.finish().map_err(RunsError::Fragmented)?;
    Ok(FileRuns { ext, len, seal })
}

/// The longest file any Chimera write makes: the bench's 4 MiB card test
/// file (a `put` is at most 1 MiB, a project about 175 KB). A cut strands
/// at most one file's chain and one directory cluster, so it bounds what
/// one sweep may free (ADR 0094).
pub const MAX_CHAIN_BYTES: u32 = 4 << 20;

/// The card's sweep mark, `/CHIMERA/SWEEP.MRK` (ADR 0094). Every change
/// that writes turns a clean one dirty before its first write and back
/// after its last; a cut in between leaves it dirty, and the next boot
/// sweeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// Every change since the last settled sweep finished.
    Clean,
    /// A change began and never finished, or a sweep left work or a
    /// doubt; or a block that isn't exactly the clean one.
    Dirty,
    /// `/CHIMERA` with no mark: written before marks, or the mark's own
    /// making was cut.
    Missing,
    /// No `/CHIMERA`: Chimera never wrote the card.
    Unused,
    /// Dirty, and the last this many boots' sweeps, in a row, doubted
    /// (1..=`DOUBT_HALT`).
    Doubted(u8),
    /// A sweep asked for (the console's `sweep`, STORAGE › CLEAN UP): the
    /// next boot sweeps, halted or not, then puts back the doubt count it
    /// carries (0..=`DOUBT_HALT`) if it doubts too.
    Asked(u8),
}

/// Doubting boots in a row after which the boot stops sweeping and warns
/// (owner ruling 2026-10-08). A settled sweep, the console's included,
/// clears the mark; a boot that runs without a doubt ends the streak.
pub const DOUBT_HALT: u8 = 3;

impl Mark {
    /// The boot sweeps.
    pub fn owes_sweep(self) -> bool {
        match self {
            Mark::Dirty | Mark::Missing | Mark::Asked(_) => true,
            Mark::Doubted(n) => n < DOUBT_HALT,
            Mark::Clean | Mark::Unused => false,
        }
    }

    /// The boot no longer sweeps: the card needs a check.
    pub fn halted(self) -> bool {
        matches!(self, Mark::Doubted(n) if n >= DOUBT_HALT)
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
    /// `Free` found more lost clusters, carried, than two cuts of the
    /// longest chain can strand: a mark bug, never a cut.
    TooMany(u32),
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
            Doubt::TooMany(_) => "too-many",
            Doubt::Card(StoreError::NoCard) => "no-card",
            Doubt::Card(StoreError::Timeout) => "timeout",
            Doubt::Card(StoreError::VolumeChanged(_)) => "card-changed",
            Doubt::Card(StoreError::Unsupported(_)) => "unsupported",
            // The sweep's read-only phases asked to write: a bug.
            Doubt::Card(StoreError::ReadOnly) => "write-refused",
            Doubt::Card(StoreError::Busy) => "busy",
            Doubt::Card(_) => "card-error",
        }
    }
}

/// What a boot's sweep ended with.
pub type SweepSeen = Result<Swept, (Doubt, SweepStats)>;

/// Why a sweep ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    /// At boot, the mark owing one.
    Owed(Mark),
    /// At boot, the mark (or the card) not read: owed, to be safe.
    Unread,
    /// The mark `Asked`: a sweep requested before this boot.
    Asked,
}

/// A sweep, run or skipped, as `status` and `sweep` report it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepRun {
    /// The boot's: the mark owed none.
    Skipped(Mark),
    Ran {
        mode: Sweep,
        why: Trigger,
        seen: SweepSeen,
        then: Then,
    },
}

/// What became of the mark after a sweep that ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Then {
    /// The sweep settled; the mark is clean.
    Cleared,
    /// Unsettled, a doubt, or no `/CHIMERA`: as it was.
    Kept,
    /// Settled or doubted, but the mark's write failed: still owing.
    Failed,
    /// A boot's doubt, counted: this many in a row.
    Doubted(u8),
}

impl SweepRun {
    /// The boot's sweep stopped for doubts (`Mark::halted`): `status`
    /// and the boot warn.
    pub fn halted(&self) -> bool {
        match *self {
            SweepRun::Skipped(m) => m.halted(),
            SweepRun::Ran {
                then: Then::Doubted(n),
                ..
            } => Mark::Doubted(n).halted(),
            SweepRun::Ran { .. } => false,
        }
    }
}

/// A `Store` whose operations' time can be capped: the console's `put`
/// holds each to what is left of its own deadline.
pub trait Files: Store {
    /// Each operation armed from now on ends after `ms` in all; `None`: the
    /// medium's own cap.
    fn cap(&mut self, ms: Option<u32>);
}

#[cfg(test)]
mod runs_tests {
    use super::*;

    fn bc(n: u32) -> BlockCount {
        BlockCount::new(n).unwrap()
    }

    #[test]
    fn thirty_three_runs_are_fragmented() {
        let mut r = Runs::new();
        for i in 0..MAX_RUNS as u32 + 1 {
            r.push(Lba::new(i * 10), bc(1));
        }
        assert_eq!(r.finish().err(), Some(Fragmented { runs: 33 }));

        let mut r = Runs::new();
        for i in 0..MAX_RUNS as u32 {
            r.push(Lba::new(i * 10), bc(2));
        }
        let e = r.finish().unwrap();
        assert_eq!(e.runs(), 32);
        // Block 3 is the second block of run 1; block 63 the last of run 31.
        assert_eq!(e.locate(FileBlock::new(3)), Some((Lba::new(11), bc(1))));
        assert_eq!(e.locate(FileBlock::new(63)), Some((Lba::new(311), bc(1))));
        assert_eq!(e.locate(FileBlock::new(64)), None);
    }

    #[test]
    fn transfers_never_cross_a_run() {
        let mut r = Runs::new();
        r.push(Lba::new(1_000), bc(10));
        r.push(Lba::new(5_000), bc(6));
        let e = r.finish().unwrap();
        let mut t = e.transfers(FileBlock::new(4), 12, bc(4));
        for want in [(1_004, 4), (1_008, 2), (5_000, 4), (5_004, 2)] {
            let (a, n) = t.next().unwrap();
            assert_eq!((a.get(), n.get()), want);
        }
        assert!(t.next().is_none());
        assert_eq!(e.first(), Lba::new(1_000));
        let n: u32 = e
            .transfers(FileBlock::new(14), 10, bc(32))
            .map(|(_, n)| n.get())
            .sum();
        assert_eq!(n, 2, "stops short past the last run");
    }

    #[test]
    fn synthetic_files_do_not_overlap_under_the_stride() {
        let a = FileName::new(Dir::Tapes, b"A", b"").unwrap();
        let b = FileName::new(Dir::Tapes, b"B", b"").unwrap();
        let (la, lb) = (synthetic_lba(a).get(), synthetic_lba(b).get());
        assert!(la.abs_diff(lb) >= SYNTHETIC_STRIDE);
    }
}
