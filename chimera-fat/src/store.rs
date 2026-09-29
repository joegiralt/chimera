//! `Store` over `embedded-sdmmc`: the card is mounted per operation, every
//! handle is closed on every path, and a boot sector the library can't read
//! safely never reaches it.

use crate::volume::{Layout, Link, Root, first_partition, layout};
use chimera_hal::store::{ByteSink, CHUNK, Dir, FileName, ReadSink, Store, StoreError, VolumeId};
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::SpiDevice;
use embedded_sdmmc::{
    Block, BlockDevice, BlockIdx, DirEntry, Error, Mode, RawDirectory, RawFile, SdCard,
    SdCardError, ShortFileName, TimeSource, Timestamp, VolumeIdx, VolumeManager,
};

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
    /// A fault seen since `start_op`, which the library may have hidden
    /// (it reports any failed cluster allocation as `DiskFull`).
    fn fault(&self) -> Option<StoreError> {
        None
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
            | SdCardError::TimeoutACommand(_) => self.fault().unwrap_or(StoreError::Io),
            _ => StoreError::Io,
        }
    }

    fn fault(&self) -> Option<StoreError> {
        self.spi(|s| {
            s.timed_out().then_some(match s.phase() {
                BusPhase::Acquire => StoreError::NoCard,
                BusPhase::Data => StoreError::Timeout,
            })
        })
    }
}

/// 2026-01-01 00:00: there is no clock to stamp files with.
pub struct FixedTime;

impl TimeSource for FixedTime {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 0,
            zero_indexed_day: 0,
            hours: 0,
            minutes: 0,
            seconds: 0,
        }
    }
}

/// Root, `CHIMERA`, and `PROJECTS` or `SOUNDS`; one file at a time.
const MAX_DIRS: usize = 3;
const MAX_FILES: usize = 1;
const MAX_VOLUMES: usize = 1;
/// Handle ids start here, as in `VolumeManager::new`.
const ID_OFFSET: u32 = 5000;

type Vm<D, T> = VolumeManager<D, T, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;

/// The mounted partition: where it starts and its `Layout`.
struct Fat {
    lba: u32,
    layout: Layout,
}

/// A FAT16/FAT32 card behind `Store`.
pub struct FatStore<D: Medium, T: TimeSource> {
    vm: Vm<D, T>,
}

/// The directory names, root first.
fn path(dir: Dir) -> &'static [&'static str] {
    match dir {
        Dir::Chimera => &["CHIMERA"],
        Dir::Projects => &["CHIMERA", "PROJECTS"],
        Dir::Sounds => &["CHIMERA", "SOUNDS"],
    }
}

fn short_name(file: &FileName) -> Result<ShortFileName, StoreError> {
    let mut buf = [0u8; 12];
    let (stem, ext) = (file.stem(), file.ext());
    buf[..stem.len()].copy_from_slice(stem);
    let mut n = stem.len();
    if !ext.is_empty() {
        buf[n] = b'.';
        buf[n + 1..n + 1 + ext.len()].copy_from_slice(ext);
        n += 1 + ext.len();
    }
    // A-Z 0-9 by `FileName`'s construction: neither step fails.
    let s = core::str::from_utf8(&buf[..n]).map_err(|_| StoreError::Io)?;
    ShortFileName::create_from_str(s).map_err(|_| StoreError::Io)
}

impl<D: Medium, T: TimeSource> FatStore<D, T> {
    pub fn new(dev: D, time: T) -> Self {
        Self {
            vm: VolumeManager::new_with_limits(dev, time, ID_OFFSET),
        }
    }

    /// A directory or file is open. (A leaked volume fails the next
    /// operation's open instead.)
    pub fn has_open_handles(&self) -> bool {
        self.vm.has_open_handles()
    }

    /// Maps a library error, re-initing the medium after a card fault.
    fn fail(&self, e: Error<D::Error>) -> StoreError {
        let (err, reinit) = match e {
            Error::DeviceError(e) => (self.vm.device(|d| d.classify(&e)), true),
            Error::NotEnoughSpace | Error::DiskFull => match self.vm.device(|d| d.fault()) {
                Some(f) => (f, true),
                None => (StoreError::Full, false),
            },
            // A directory under a file's name is no such file.
            Error::NotFound | Error::OpenedDirAsFile => (StoreError::NotFound, false),
            // `read` stops at the entry's length, so an end of chain before
            // it is a short chain.
            Error::EndOfFile | Error::BadCluster | Error::UnterminatedFatChain => {
                (StoreError::Corrupt, false)
            }
            _ => (StoreError::Io, true),
        };
        if reinit {
            self.vm.device(|d| d.reinit());
        }
        err
    }

    /// `r`, unless it is `Ok` and the close after it failed.
    fn closed<R>(
        &self,
        r: Result<R, StoreError>,
        close: Result<(), Error<D::Error>>,
    ) -> Result<R, StoreError> {
        let v = r?;
        close.map_err(|e| self.fail(e))?;
        Ok(v)
    }

    /// Straight from the medium, bypassing (and dropping) the library's
    /// block cache, which may hold a block from a card since swapped.
    fn read_block(&self, idx: u32) -> Result<[u8; 512], D::Error> {
        let mut b = [Block::new()];
        self.vm.device(|d| d.read(&mut b, BlockIdx(idx)))?;
        Ok(b[0].contents)
    }

    fn read_fs_block(&self, idx: u32) -> Result<[u8; 512], StoreError> {
        self.read_block(idx)
            .map_err(|e| self.fail(Error::DeviceError(e)))
    }

    /// The volume's id and layout, from the boot sector, if the library can
    /// read it safely.
    fn identify(&self, mbr: &[u8; 512]) -> Result<(VolumeId, Fat), StoreError> {
        let p = first_partition(mbr).map_err(StoreError::Unsupported)?;
        let bs = self.read_fs_block(p.lba)?;
        let (layout, id) = layout(&bs, p).map_err(StoreError::Unsupported)?;
        Ok((id, Fat { lba: p.lba, layout }))
    }

    /// `Corrupt` unless the chain from `start` stays on the volume and ends,
    /// holding at least `need` bytes. The library would mis-walk the rest: a
    /// free or reserved link sends it before the data area (a panic in debug
    /// builds), a loop never ends.
    fn check_chain(&self, fat: &Fat, start: u32, need: u32) -> Result<(), StoreError> {
        let lay = &fat.layout;
        let mut cached: Option<(u32, [u8; 512])> = None;
        let (mut c, mut n) = (start, 1);
        loop {
            if !lay.holds(c) {
                return Err(StoreError::Corrupt);
            }
            let idx = fat.lba.saturating_add(lay.fat_block(c));
            let block = match cached {
                Some((i, b)) if i == idx => b,
                _ => {
                    let b = self.read_fs_block(idx)?;
                    cached = Some((idx, b));
                    b
                }
            };
            match lay.link(&block, c) {
                Link::End => break,
                Link::Next(next) if n < lay.clusters() => (c, n) = (next, n + 1),
                Link::Next(_) | Link::Broken => return Err(StoreError::Corrupt),
            }
        }
        if u64::from(n) * u64::from(lay.cluster_bytes()) < u64::from(need) {
            return Err(StoreError::Corrupt);
        }
        Ok(())
    }

    /// `check_chain` for a directory entry's chain; `whole`: it must hold
    /// the entry's length (a read), not only be safe to walk (a truncate).
    fn check_entry(&self, fat: &Fat, e: &DirEntry, whole: bool) -> Result<(), StoreError> {
        let block = self.read_fs_block(e.entry_block.0)?;
        let start = fat.layout.start_cluster(&block, e.entry_offset as usize);
        let need = if whole { e.size } else { 0 };
        // An empty file has no chain; a directory always has one.
        if start == 0 && !e.attributes.is_directory() && need == 0 {
            return Ok(());
        }
        self.check_chain(fat, start, need)
    }

    /// Checks the card is `vol`, then runs `f` on `path`'s last directory,
    /// with the volume and every directory up to it open, and closes them
    /// all in reverse.
    fn with_dir<R>(
        &self,
        vol: VolumeId,
        path: &[&str],
        f: impl FnOnce(&Self, &Fat, RawDirectory) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        self.vm.device(|d| d.start_op());
        let mbr = self.read_fs_block(0)?;
        let (now, fat) = self.identify(&mbr)?;
        if now != vol {
            return Err(StoreError::VolumeChanged(now));
        }
        if let Root::Cluster(root) = fat.layout.root() {
            self.check_chain(&fat, root, 0)?;
        }
        let v = self
            .vm
            .open_raw_volume(VolumeIdx(0))
            .map_err(|e| self.fail(e))?;
        let r = match self.vm.open_root_dir(v) {
            Ok(root) => {
                let r = self.walk(&fat, root, path, f);
                self.closed(r, self.vm.close_dir(root))
            }
            Err(e) => Err(self.fail(e)),
        };
        self.closed(r, self.vm.close_volume(v))
    }

    fn walk<R>(
        &self,
        fat: &Fat,
        dir: RawDirectory,
        path: &[&str],
        f: impl FnOnce(&Self, &Fat, RawDirectory) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let Some((name, rest)) = path.split_first() else {
            return f(self, fat, dir);
        };
        let e = self
            .vm
            .find_directory_entry(dir, *name)
            .map_err(|e| self.fail(e))?;
        self.check_entry(fat, &e, false)?;
        let sub = self.vm.open_dir(dir, *name).map_err(|e| self.fail(e))?;
        let r = self.walk(fat, sub, rest, f);
        self.closed(r, self.vm.close_dir(sub))
    }

    /// Runs `f` on `name` opened in `dir`, then closes it.
    fn with_file<R>(
        &self,
        dir: RawDirectory,
        name: ShortFileName,
        mode: Mode,
        f: impl FnOnce(RawFile) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let file = self
            .vm
            .open_file_in_dir(dir, name, mode)
            .map_err(|e| self.fail(e))?;
        let r = f(file);
        self.closed(r, self.vm.close_file(file))
    }
}

/// `write`'s `ByteSink`: whole blocks to the file, the tail on `finish`.
struct BlockSink<'a, D: Medium, T: TimeSource> {
    store: &'a FatStore<D, T>,
    file: RawFile,
    buf: [u8; CHUNK],
    len: usize,
    total: u32,
    /// The first device error; later puts repeat it without touching the card.
    failed: Option<StoreError>,
}

impl<D: Medium, T: TimeSource> BlockSink<'_, D, T> {
    fn flush(&mut self) -> Result<(), StoreError> {
        if let Some(e) = self.failed {
            return Err(e);
        }
        if self.len > 0 {
            if let Err(e) = self.store.vm.write(self.file, &self.buf[..self.len]) {
                let e = self.store.fail(e);
                self.failed = Some(e);
                return Err(e);
            }
            self.len = 0;
        }
        Ok(())
    }
}

impl<D: Medium, T: TimeSource> ByteSink for BlockSink<'_, D, T> {
    fn put(&mut self, mut bytes: &[u8]) -> Result<(), StoreError> {
        while !bytes.is_empty() {
            if let Some(e) = self.failed {
                return Err(e);
            }
            let n = (CHUNK - self.len).min(bytes.len());
            self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
            (self.len, bytes) = (self.len + n, &bytes[n..]);
            self.total += n as u32;
            if self.len == CHUNK {
                self.flush()?;
            }
        }
        Ok(())
    }
}

impl<D: Medium, T: TimeSource> Store for FatStore<D, T> {
    fn mount(&mut self) -> Result<VolumeId, StoreError> {
        self.vm.device(|d| d.start_op());
        let mbr = match self.read_block(0) {
            Ok(b) => b,
            // A card swapped while idle is still in SD mode: re-init, retry once.
            Err(_) => {
                self.vm.device(|d| {
                    d.reinit();
                    d.start_op();
                });
                self.read_block(0)
                    .map_err(|e| self.fail(Error::DeviceError(e)))?
            }
        };
        let (id, _) = self.identify(&mbr)?;
        let v = self
            .vm
            .open_raw_volume(VolumeIdx(0))
            .map_err(|e| self.fail(e))?;
        self.vm.close_volume(v).map_err(|e| self.fail(e))?;
        self.vm.device(|d| d.mounted());
        Ok(id)
    }

    fn list(
        &mut self,
        vol: VolumeId,
        dir: Dir,
        f: &mut dyn FnMut(FileName, u32),
    ) -> Result<(), StoreError> {
        self.with_dir(vol, path(dir), |s, _, d| {
            s.vm.iterate_dir(d, |e| {
                let name = &e.name;
                if !e.attributes.is_directory()
                    && !e.attributes.is_volume()
                    && let Some(n) = FileName::new(dir, name.base_name(), name.extension())
                {
                    f(n, e.size);
                }
                core::ops::ControlFlow::Continue(())
            })
            .map_err(|e| s.fail(e))
        })
    }

    fn read(
        &mut self,
        vol: VolumeId,
        file: FileName,
        sink: &mut dyn ReadSink,
    ) -> Result<(), StoreError> {
        let name = short_name(&file)?;
        self.with_dir(vol, path(file.dir()), |s, fat, d| {
            let e = s.vm.find_directory_entry(d, name).map_err(|e| s.fail(e))?;
            s.check_entry(fat, &e, true)?;
            s.with_file(d, name, Mode::ReadOnly, |f| {
                let len = s.vm.file_length(f).map_err(|e| s.fail(e))?;
                if sink.begin(len).is_break() {
                    return Ok(());
                }
                let mut buf = [0u8; CHUNK];
                loop {
                    let n = s.vm.read(f, &mut buf).map_err(|e| s.fail(e))?;
                    if n == 0 || sink.chunk(&buf[..n]).is_break() {
                        return Ok(());
                    }
                }
            })
        })
    }

    fn write(
        &mut self,
        vol: VolumeId,
        file: FileName,
        body: &mut dyn FnMut(&mut dyn ByteSink) -> Result<(), StoreError>,
    ) -> Result<u32, StoreError> {
        let name = short_name(&file)?;
        self.with_dir(vol, path(file.dir()), |s, fat, d| {
            // Truncating walks the old chain.
            match s.vm.find_directory_entry(d, name) {
                Ok(e) => s.check_entry(fat, &e, false)?,
                Err(Error::NotFound) => {}
                Err(e) => return Err(s.fail(e)),
            }
            s.with_file(d, name, Mode::ReadWriteCreateOrTruncate, |f| {
                let mut sink = BlockSink {
                    store: s,
                    file: f,
                    buf: [0; CHUNK],
                    len: 0,
                    total: 0,
                    failed: None,
                };
                let r = body(&mut sink);
                // What `body` put stays, even when it fails; a card fault
                // outranks the body's own error.
                sink.flush()?;
                r?;
                Ok(sink.total)
            })
        })
    }

    fn delete(&mut self, vol: VolumeId, file: FileName) -> Result<(), StoreError> {
        let name = short_name(&file)?;
        self.with_dir(vol, path(file.dir()), |s, _, d| {
            let e = s.vm.find_directory_entry(d, name).map_err(|e| s.fail(e))?;
            // The library would delete an empty directory by this name.
            if e.attributes.is_directory() {
                return Err(StoreError::NotFound);
            }
            s.vm.delete_entry_in_dir(d, name).map_err(|e| s.fail(e))
        })
    }

    fn make_dir(&mut self, vol: VolumeId, dir: Dir) -> Result<(), StoreError> {
        let Some((name, parent)) = path(dir).split_last() else {
            return Ok(());
        };
        self.with_dir(vol, parent, |s, _, d| {
            match s.vm.make_dir_in_dir(d, *name) {
                Ok(()) | Err(Error::DirAlreadyExists) => Ok(()),
                Err(e) => Err(s.fail(e)),
            }
        })
    }
}
