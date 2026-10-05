//! `Store` over the FAT core: the card is mounted per operation, its
//! `VolumeId` checked, and every block bounded to the partition. Errors are
//! mapped here, and a device error re-inits the card.

use crate::blocks::{BLOCK, Blocks, FsError};
use crate::fat::{FatCache, Orphan, Stale, Table};
use crate::fs::Fs;
use crate::sweep::{self, Fail};
use crate::volume::{Layout, first_partition, layout};
use chimera_hal::store::{
    ByteSink, Dir, Doubt, FileName, Files, ReadSink, Store, StoreError, Sweep, SweepSeen,
    SweepStats, Swept, VolumeId,
};
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiDevice;
use embedded_sdmmc::{Block, BlockDevice, BlockIdx, SdCard, SdCardError};

/// A block device `FatStore` can drive: it arms deadlines, re-inits after
/// errors and classifies its own faults.
pub trait Medium: BlockDevice {
    /// Arm the medium's deadline for one `Store` operation.
    fn start_op(&self) {}
    /// After an error: wake, slow clock, re-acquire on the next access.
    fn reinit(&self) {}
    /// The volume opened: fast clock.
    fn mounted(&self) {}
    /// Each operation armed from now on ends after `ms` in all; `None`:
    /// the medium's own cap.
    fn cap(&self, _ms: Option<u32>) {}
    fn classify(&self, _e: &Self::Error) -> StoreError {
        StoreError::Io
    }
    /// Ms since this operation's `start_op`, as of its last transaction:
    /// the sweep's phase times. 0 where nothing counts.
    fn elapsed_ms(&self) -> u32 {
        0
    }
}

/// Where the SD bus is: acquiring at the init clock under `SD_ACQUIRE_MS`,
/// or moving data at the fast clock under `SD_IDLE_MS`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BusPhase {
    Acquire,
    Data,
}

/// The SD SPI bus's clock and deadline, beside its `SpiDevice`.
pub trait SdBus {
    /// ≥ 74 clocks with CS high, at the init clock.
    fn wake(&mut self);
    fn set_phase(&mut self, p: BusPhase);
    fn phase(&self) -> BusPhase;
    /// Arm the phase's deadline.
    fn start_op(&mut self);
    /// `Medium::cap`.
    fn set_cap(&mut self, _ms: Option<u32>) {}
    fn timed_out(&self) -> bool;
    /// `Medium::elapsed_ms`.
    fn elapsed_ms(&self) -> u32 {
        0
    }
}

/// The deadline, if it passed: no card while acquiring, a timeout after.
fn deadline<S: SpiDevice<u8> + SdBus, D: DelayNs>(sd: &SdCard<S, D>) -> Option<StoreError> {
    sd.spi(|s| {
        s.timed_out().then_some(match s.phase() {
            BusPhase::Acquire => StoreError::NoCard,
            BusPhase::Data => StoreError::Timeout,
        })
    })
}

impl<S: SpiDevice<u8> + SdBus, D: DelayNs> Medium for SdCard<S, D> {
    fn start_op(&self) {
        self.spi(|s| s.start_op());
    }

    fn reinit(&self) {
        self.spi(|s| {
            s.set_phase(BusPhase::Acquire);
            s.wake();
        });
        self.mark_card_uninit();
    }

    fn mounted(&self) {
        self.spi(|s| s.set_phase(BusPhase::Data));
    }

    fn cap(&self, ms: Option<u32>) {
        self.spi(|s| s.set_cap(ms));
    }

    fn elapsed_ms(&self) -> u32 {
        self.spi(|s| s.elapsed_ms())
    }

    fn classify(&self, e: &SdCardError) -> StoreError {
        match e {
            SdCardError::CardNotFound => StoreError::NoCard,
            SdCardError::Transport
            | SdCardError::TimeoutReadBuffer
            | SdCardError::TimeoutWaitNotBusy
            | SdCardError::TimeoutCommand(_)
            | SdCardError::TimeoutACommand(_) => deadline(self).unwrap_or(StoreError::Io),
            _ => StoreError::Io,
        }
    }
}

/// Why a partition block access failed.
enum PartError<E> {
    Dev(E),
    /// At or past the volume's end: never sent to the device.
    Outside,
}

/// The mounted volume's blocks, numbered from its boot sector.
struct Part<'a, D> {
    dev: &'a D,
    lba: u32,
    blocks: u32,
}

impl<D: BlockDevice> Part<'_, D> {
    fn at(&self, lba: u32) -> Result<BlockIdx, PartError<D::Error>> {
        (lba < self.blocks)
            .then(|| self.lba.checked_add(lba))
            .flatten()
            .map(BlockIdx)
            .ok_or(PartError::Outside)
    }
}

// The device takes `Block`s, which aren't `[u8; 512]`: each access copies
// through one on the stack.
impl<D: BlockDevice> Blocks for Part<'_, D> {
    type Error = PartError<D::Error>;

    fn read(&mut self, lba: u32, buf: &mut [u8; BLOCK]) -> Result<(), Self::Error> {
        let at = self.at(lba)?;
        let mut b = [Block::new()];
        self.dev.read(&mut b, at).map_err(PartError::Dev)?;
        *buf = b[0].contents;
        Ok(())
    }

    fn write(&mut self, lba: u32, buf: &[u8; BLOCK]) -> Result<(), Self::Error> {
        let at = self.at(lba)?;
        self.dev
            .write(&[Block { contents: *buf }], at)
            .map_err(PartError::Dev)
    }
}

/// Why a `ReadOnly` access failed.
enum RoError<E> {
    Dev(E),
    /// A write: never sent.
    Refused,
}

/// The sweep's blocks while it only reads, and for all of a dry run:
/// every write is refused before it reaches the card.
struct ReadOnly<B>(B);

impl<B: Blocks> Blocks for ReadOnly<B> {
    type Error = RoError<B::Error>;

    fn read(&mut self, lba: u32, buf: &mut [u8; BLOCK]) -> Result<(), Self::Error> {
        self.0.read(lba, buf).map_err(RoError::Dev)
    }

    /// Never asked: the read phases don't write. A bug, if it is.
    fn write(&mut self, _lba: u32, _buf: &[u8; BLOCK]) -> Result<(), Self::Error> {
        debug_assert!(false, "a write in the sweep's read phases");
        Err(RoError::Refused)
    }
}

/// A value that holds only for the volume it was taken on.
struct Keyed<T>(Option<(VolumeId, T)>);

impl<T> Keyed<T> {
    const fn new() -> Self {
        Self(None)
    }

    /// Takes the value if it is `id`'s; another volume's is dropped.
    fn for_volume(&mut self, id: VolumeId) -> Option<T> {
        self.0.take().filter(|(of, _)| *of == id).map(|(_, v)| v)
    }

    fn set(&mut self, id: VolumeId, v: T) {
        self.0 = Some((id, v));
    }
}

/// A FAT16/FAT32 card behind `Store`. Its RAM: one block buffer, the FAT
/// cache, and, each for the volume it was taken on, the allocator's hint
/// a FAT sector FAT 2 missed, and a chain a failed write left.
pub struct FatStore<D: Medium> {
    dev: D,
    buf: [u8; BLOCK],
    fat: FatCache,
    hint: Keyed<u32>,
    stale: Keyed<Stale>,
    orphan: Keyed<Orphan>,
}

type FsResult<T, D> = Result<T, FsError<PartError<<D as BlockDevice>::Error>>>;

impl<D: Medium> FatStore<D> {
    pub fn new(dev: D) -> Self {
        Self {
            dev,
            buf: [0; BLOCK],
            fat: FatCache::new(),
            hint: Keyed::new(),
            stale: Keyed::new(),
            orphan: Keyed::new(),
        }
    }

    pub fn device(&self) -> &D {
        &self.dev
    }

    /// `Fs::remove_dir` on the card, if it is still `vol`.
    pub fn remove_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<bool, StoreError> {
        self.run(vol, |fs, _| fs.remove_dir(dir))
    }

    /// `Fs::free_bytes` on the card, if it is still `vol`.
    pub fn free_bytes(&mut self, vol: VolumeId) -> Result<u64, StoreError> {
        self.run(vol, |fs, _| fs.free_bytes())
    }

    /// `Fs::aim` on the card, if it is still `vol`: kept for its next write.
    pub fn aim(&mut self, vol: VolumeId, per_mille: u16) -> Result<(), StoreError> {
        self.run(vol, |fs, _| {
            fs.aim(per_mille);
            Ok(())
        })
    }

    /// `Fs::runs` on the card, if it is still `vol`, each run's first block
    /// on the device: for raw reads that bypass the FAT.
    pub fn runs(
        &mut self,
        vol: VolumeId,
        file: FileName,
        f: &mut dyn FnMut(u32, u32),
    ) -> Result<u32, StoreError> {
        self.run(vol, |fs, lba| {
            fs.runs(file, &mut |at, n| f(lba.saturating_add(at), n))
        })
    }

    /// The lost-cluster sweep (#360): `Store::sweep`, one operation under
    /// the card's deadlines. `DryRun` reads through `ReadOnly` and never
    /// heals: no stale sector or orphan is touched, no FSInfo patched.
    pub fn sweep(&mut self, vol: VolumeId, mode: Sweep, marks: &mut [u8]) -> SweepSeen {
        let mut stats = SweepStats::default();
        self.dev.start_op();
        if let Err(e) = self.read(0) {
            return Err((self.read_doubt(&e), stats));
        }
        let (lba, layout, now) = self.identify().map_err(|e| (Doubt::Card(e), stats))?;
        if now != vol {
            return Err((Doubt::Card(StoreError::VolumeChanged(now)), stats));
        }
        stats.spc = layout.blocks_per_cluster();
        stats.clusters = layout.clusters();
        stats.fat_bytes = layout.fat1().len() as u32 * BLOCK as u32;
        if !sweep::has_room(&layout, marks) {
            return Err((Doubt::NoRoom, stats));
        }
        let mut ro = ReadOnly(Part {
            dev: &self.dev,
            lba,
            blocks: layout.blocks(),
        });
        // FAT 2's copy of the one sector it differs in.
        let mut d = [0; BLOCK];
        let dev = &self.dev;
        let found = sweep::survey(
            &mut Table::new(&mut ro, &layout, &mut self.fat),
            &mut self.buf,
            marks,
            &mut d,
            &|| dev.elapsed_ms(),
            &mut stats,
        );
        let found = match found {
            Ok(f) => f,
            Err(Fail::Doubt(d)) => return Err((d, stats)),
            Err(Fail::Dev(RoError::Dev(PartError::Dev(e)))) => {
                return Err((self.read_doubt(&e), stats));
            }
            Err(Fail::Dev(RoError::Dev(PartError::Outside))) => {
                return Err((Doubt::Card(StoreError::Corrupt), stats));
            }
            Err(Fail::Dev(RoError::Refused)) => {
                return Err((Doubt::Card(StoreError::ReadOnly), stats));
            }
        };
        let mut swept = Swept {
            lost: found.lost,
            freed: 0,
            healed: found.differs.is_some(),
            stats,
        };
        if mode == Sweep::DryRun || (found.lost == 0 && found.differs.is_none()) {
            return Ok(swept);
        }
        let ReadOnly(mut part) = ro;
        let freed = sweep::free(
            &mut part,
            &layout,
            &mut self.fat,
            &mut self.buf,
            marks,
            &found,
            &d,
        );
        if let Some(s) = self.fat.take_stale() {
            self.stale.set(now, s);
        }
        swept.freed = freed.map_err(|e| {
            let e = match e {
                FsError::Dev(PartError::Dev(e)) => self.fault(&e),
                _ => StoreError::Corrupt,
            };
            (Doubt::Card(e), stats)
        })?;
        Ok(swept)
    }

    /// A device error while the sweep reads: `Io` is `Unreadable`; a
    /// timeout, an empty slot or a swap is the card's.
    fn read_doubt(&self, e: &D::Error) -> Doubt {
        match self.fault(e) {
            StoreError::Io => Doubt::Unreadable,
            e => Doubt::Card(e),
        }
    }

    /// A device error: classified by the medium, which re-inits.
    fn fault(&self, e: &D::Error) -> StoreError {
        let err = self.dev.classify(e);
        self.dev.reinit();
        err
    }

    /// Reads absolute block `lba` into `buf`.
    fn read(&mut self, lba: u32) -> Result<(), D::Error> {
        let mut b = [Block::new()];
        self.dev.read(&mut b, BlockIdx(lba))?;
        self.buf = b[0].contents;
        Ok(())
    }

    /// The volume behind the MBR in `buf`: where it starts, its layout and
    /// its id.
    fn identify(&mut self) -> Result<(u32, Layout, VolumeId), StoreError> {
        let p = first_partition(&self.buf).map_err(StoreError::Unsupported)?;
        self.read(p.lba).map_err(|e| self.fault(&e))?;
        let (l, id) = layout(&self.buf, p).map_err(StoreError::Unsupported)?;
        Ok((p.lba, l, id))
    }

    /// Runs `op` on the card, if it is still `vol`, with the partition's
    /// first block.
    ///
    /// Error mapping. `FsError::Dev` is fatal for the whole operation: the
    /// `Fs` has stopped where it failed and dropped any unflushed FAT
    /// change, and nothing here retries, since a failed flush empties the
    /// FAT cache and a retry would lose the change without a word. It maps
    /// to the medium's class (`NoCard`, `Timeout` or `Io`) and re-inits the
    /// card, so the next operation reads it afresh. `Outside` can't happen
    /// while the core keeps its bounds; if it does, the volume lied:
    /// `Corrupt`. The rest map one to one, `Body(e)` to `e`.
    fn run<T>(
        &mut self,
        vol: VolumeId,
        op: impl FnOnce(&mut Fs<'_, Part<'_, D>>, u32) -> FsResult<T, D>,
    ) -> Result<T, StoreError> {
        self.dev.start_op();
        self.read(0).map_err(|e| self.fault(&e))?;
        let (lba, layout, now) = self.identify()?;
        if now != vol {
            return Err(StoreError::VolumeChanged(now));
        }
        let mut hint = self.hint.for_volume(now);
        self.fat.keep_stale(self.stale.for_volume(now));
        self.fat.keep_orphan(self.orphan.for_volume(now));
        let mut part = Part {
            dev: &self.dev,
            lba,
            blocks: layout.blocks(),
        };
        let r = op(
            &mut Fs::new(&mut part, layout, &mut self.fat, &mut self.buf, &mut hint),
            lba,
        );
        if let Some(c) = hint {
            self.hint.set(now, c);
        }
        if let Some(c) = self.fat.take_orphan() {
            self.orphan.set(now, c);
        }
        if let Some(s) = self.fat.take_stale() {
            self.stale.set(now, s);
        }
        r.map_err(|e| match e {
            FsError::Dev(PartError::Dev(e)) => self.fault(&e),
            FsError::Dev(PartError::Outside) | FsError::Corrupt => StoreError::Corrupt,
            FsError::NotFound => StoreError::NotFound,
            FsError::Full => StoreError::Full,
            FsError::Body(e) => e,
        })
    }
}

impl<D: Medium> Store for FatStore<D> {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.dev.start_op();
        if self.read(0).is_err() {
            // A card swapped while idle is still in SD mode: re-init, retry once.
            self.dev.reinit();
            self.dev.start_op();
            self.read(0).map_err(|e| self.fault(&e))?;
        }
        // No volume to use (unsupported, exFAT): back to acquiring, so the
        // medium's phase and the driver agree the card needs re-init.
        let (_, _, id) = self.identify().inspect_err(|_| self.dev.reinit())?;
        self.dev.mounted();
        Ok(id)
    }

    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.run(vol, |fs, _| fs.list(dir, f))
    }

    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.run(vol, |fs, _| fs.read(file, sink))
    }

    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.run(vol, |fs, _| fs.write(file, body))
    }

    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.run(vol, |fs, _| fs.delete(file))
    }

    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.run(vol, |fs, _| fs.make_dir(dir))
    }

    fn sweep(&mut self, vol: VolumeId, mode: Sweep, marks: &mut [u8]) -> SweepSeen {
        FatStore::sweep(self, vol, mode, marks)
    }
}

impl<D: Medium> Files for FatStore<D> {
    fn cap(&mut self, ms: Option<u32>) {
        self.dev.cap(ms);
    }
}
