//! `Store` over the FAT core: the card is mounted per operation, its
//! `VolumeId` checked, and every block bounded to the partition. Errors are
//! mapped here, and a device error re-inits the card.

use crate::blocks::{BLOCK, Blocks, FsError};
use crate::fat::FatCache;
use crate::fs::Fs;
use crate::volume::{Layout, first_partition, layout};
use chimera_hal::store::{ByteSink, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
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
    fn classify(&self, _e: &Self::Error) -> StoreError {
        StoreError::Io
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
    fn timed_out(&self) -> bool;
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

/// A FAT16/FAT32 card behind `Store`. Its RAM: one block buffer, the FAT
/// cache, and the allocator's hint for the card it was taken on.
pub struct FatStore<D: Medium> {
    dev: D,
    buf: [u8; BLOCK],
    fat: FatCache,
    hint: Option<(VolumeId, u32)>,
}

type FsResult<T, D> = Result<T, FsError<PartError<<D as BlockDevice>::Error>>>;

impl<D: Medium> FatStore<D> {
    pub fn new(dev: D) -> Self {
        Self {
            dev,
            buf: [0; BLOCK],
            fat: FatCache::new(),
            hint: None,
        }
    }

    pub fn device(&self) -> &D {
        &self.dev
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

    /// Runs `op` on the card, if it is still `vol`.
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
        op: impl FnOnce(&mut Fs<'_, Part<'_, D>>) -> FsResult<T, D>,
    ) -> Result<T, StoreError> {
        self.dev.start_op();
        self.read(0).map_err(|e| self.fault(&e))?;
        let (lba, layout, now) = self.identify()?;
        if now != vol {
            return Err(StoreError::VolumeChanged(now));
        }
        let mut hint = self.hint.filter(|&(id, _)| id == now).map(|(_, c)| c);
        let mut part = Part {
            dev: &self.dev,
            lba,
            blocks: layout.blocks(),
        };
        let r = op(&mut Fs::new(
            &mut part,
            layout,
            &mut self.fat,
            &mut self.buf,
            &mut hint,
        ));
        self.hint = hint.map(|c| (now, c));
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
        let (_, _, id) = self.identify()?;
        self.dev.mounted();
        Ok(id)
    }

    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.run(vol, |fs| fs.list(dir, f))
    }

    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        self.run(vol, |fs| fs.read(file, sink))
    }

    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        self.run(vol, |fs| fs.write(file, body))
    }

    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        self.run(vol, |fs| fs.delete(file))
    }

    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        self.run(vol, |fs| fs.make_dir(dir))
    }
}
