//! The console's `card` on the unit (bench builds): the SD driver's own
//! multi-block reads over two test files' runs, each read timed by the
//! cycle counter, from the UI loop while audio runs. Every read keeps
//! `SD_IDLE_MS` and the whole run `DEADLINE_MS`; audio, MIDI and the
//! watchdog's kicks run above it in interrupts throughout.

use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_core::boot::wait_until;
use chimera_core::console::{
    CardAnswer, CardArg, CardFail, CardSpeed, DEADLINE_MS, Extents, FILE_AIMS, FILE_BYTES, FILES,
    Files, Latency, Load, RANDOM_READS, READ_BLOCKS, ROOM_MARGIN_BYTES, SEQ_BLOCKS, SPAN_BLOCKS,
    Throughput, interleaved, random_reads, span_start, span_us,
};
use chimera_core::note_queue::{NoteEvent, NoteKind, NoteProducer};
use chimera_core::perf::load::AudioStats;
use chimera_core::triple::Reader;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_fat::Medium;
use chimera_hal::Ms;
use chimera_hal::store::{ByteSink, CHUNK, Dir, FileName, Store, StoreError, VolumeId};
use cortex_m::peripheral::DWT;
use embedded_sdmmc::{Block, BlockDevice, BlockIdx};

use crate::audio::dma::{BLOCKS, OVERRUNS};
use crate::controls::now_ms;
use crate::sd::{SD_OP_CAP_MS, SdStore};

/// The largest transfer, 16 KB.
const BUF_BLOCKS: usize = 32;
const _: () = assert!(READ_BLOCKS as usize <= BUF_BLOCKS);
/// Audio counts as running if it renders a block within this.
const LIVE_MS: u32 = 10;
/// A test file's write starts only with this long left of the deadline.
const WRITE_MIN_MS: u32 = 1_000;
/// The chord's voices start before the loaded reads are timed.
const SETTLE_US: u32 = 100_000;
/// Eight voices on channel 1, an octave and a half of C major sevenths.
const CHORD: [u8; 8] = [48, 52, 55, 59, 60, 64, 67, 71];
/// Names `card clean` deletes per listing.
const CLEAN_BATCH: usize = 8;

/// The transfer buffer and the latency samples: statics, not the stack.
struct Bufs {
    blocks: [Block; BUF_BLOCKS],
    lat: [u32; RANDOM_READS],
}

static mut BUFS: Bufs = Bufs {
    blocks: [const {
        Block {
            contents: [0; Block::LEN],
        }
    }; BUF_BLOCKS],
    lat: [0; RANDOM_READS],
};
static BUFS_TAKEN: AtomicBool = AtomicBool::new(false);

pub struct CardBench {
    bufs: &'static mut Bufs,
    notes: NoteProducer<'static>,
    cpu_hz: u32,
}

/// The one `CardBench`, over the `CARD` note source.
pub fn take(notes: NoteProducer<'static>, cpu_hz: u32) -> Option<CardBench> {
    if BUFS_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `BUFS` ever made.
    let bufs = unsafe { &mut *addr_of_mut!(BUFS) };
    Some(CardBench {
        bufs,
        notes,
        cpu_hz,
    })
}

fn test_file(i: usize) -> FileName {
    let stem = [b"CARD0", b"CARD1"][i % FILES];
    FileName::new(Dir::Bench, stem, b"BIN").expect("an 8.3 name")
}

/// The run's one deadline, on the ms clock (no wrap in 49 days).
struct Deadline(Ms);

impl Deadline {
    fn left_ms(&self) -> u32 {
        DEADLINE_MS.saturating_sub(now_ms().since(self.0))
    }

    fn check(&self) -> Result<(), CardFail> {
        if self.left_ms() == 0 {
            return Err(CardFail::Timeout);
        }
        Ok(())
    }
}

impl CardBench {
    /// `card`'s answer, all its card work done.
    pub fn answer(
        &mut self,
        arg: CardArg,
        store: &mut SdStore,
        stats: Option<&mut Reader<AudioStats>>,
    ) -> CardAnswer {
        match arg {
            CardArg::Clean => CardAnswer::Cleaned(clean(store)),
            CardArg::Run(seed) => {
                let mut s = CardSpeed {
                    seed: seed.unwrap_or_else(DWT::cycle_count),
                    ..CardSpeed::default()
                };
                let fail = self.measure(&mut s, store, stats).err();
                CardAnswer::Measured(s, fail)
            }
        }
    }

    fn measure(
        &mut self,
        s: &mut CardSpeed,
        store: &mut SdStore,
        stats: Option<&mut Reader<AudioStats>>,
    ) -> Result<(), CardFail> {
        let seen = BLOCKS.load(Ordering::Relaxed);
        let live = wait_until(LIVE_MS * (self.cpu_hz / 1_000), DWT::cycle_count, || {
            BLOCKS.load(Ordering::Relaxed) != seen
        });
        if !live {
            return Err(CardFail::NoAudio);
        }
        let overruns = OVERRUNS.load(Ordering::Relaxed);
        let due = Deadline(now_ms());
        let r = self.phases(s, store, stats, &due);
        s.overruns = OVERRUNS.load(Ordering::Relaxed).wrapping_sub(overruns);
        r
    }

    /// Each phase into `s` as it completes, so a stop keeps the earlier.
    fn phases(
        &mut self,
        s: &mut CardSpeed,
        store: &mut SdStore,
        mut stats: Option<&mut Reader<AudioStats>>,
        due: &Deadline,
    ) -> Result<(), CardFail> {
        let vol = store.mount()?;
        store.make_dir(vol, Dir::Chimera)?;
        store.make_dir(vol, Dir::Bench)?;
        let (ext, files) = test_files(store, vol, due)?;
        s.files = Some(files);
        for ((i, &n), out) in (0..).zip(&SEQ_BLOCKS).zip(&mut s.seq) {
            *out = Some(self.sequential(store, &ext[0], span_start(i), n, due)?);
        }
        self.chord(NoteKind::On(Velocity::DEFAULT));
        crate::clocks::delay_us(self.cpu_hz, SETTLE_US);
        let mut loaded = || -> Result<(), CardFail> {
            s.random = Some(self.random(store, &ext, s.seed, due)?);
            s.load = load(stats.as_deref_mut());
            s.interleaved = Some(self.interleaved(store, &ext, due)?);
            s.load = load(stats.as_deref_mut()).or(s.load);
            Ok(())
        };
        let r = loaded();
        self.chord(NoteKind::Off);
        r
    }

    fn chord(&mut self, kind: NoteKind) {
        for n in CHORD.into_iter().filter_map(MidiNote::new) {
            self.notes.push(NoteEvent {
                channel: MidiChannel::clamped(0),
                note: n,
                kind,
            });
        }
    }

    /// `SPAN_BLOCKS` from file block `from`, `max` blocks a transfer.
    fn sequential(
        &mut self,
        store: &SdStore,
        ext: &Extents,
        from: u32,
        max: u32,
        due: &Deadline,
    ) -> Result<Throughput, CardFail> {
        let mut us = 0u64;
        for (lba, n) in ext.transfers(from, SPAN_BLOCKS, max) {
            us += u64::from(self.timed_read(store, lba, n, due)?);
        }
        Ok(throughput(SPAN_BLOCKS, us))
    }

    /// `RANDOM_READS` timed reads of 4 KB across both files.
    fn random(
        &mut self,
        store: &SdStore,
        ext: &[Extents; FILES],
        seed: u32,
        due: &Deadline,
    ) -> Result<Latency, CardFail> {
        for (i, (file, at)) in random_reads(seed).enumerate() {
            let us = self.read_4k(store, &ext[file], at, due)?;
            if let Some(slot) = self.bufs.lat.get_mut(i) {
                *slot = us;
            }
        }
        Ok(Latency::of(&mut self.bufs.lat).unwrap_or_default())
    }

    /// TRON's pattern, `interleaved()`, timed as one rate.
    fn interleaved(
        &mut self,
        store: &SdStore,
        ext: &[Extents; FILES],
        due: &Deadline,
    ) -> Result<Throughput, CardFail> {
        let (mut us, mut blocks) = (0u64, 0u32);
        for (file, at) in interleaved() {
            us += u64::from(self.read_4k(store, &ext[file], at, due)?);
            blocks += READ_BLOCKS;
        }
        Ok(throughput(blocks, us))
    }

    /// One 4 KB read from file block `at`, split where a run ends.
    fn read_4k(
        &mut self,
        store: &SdStore,
        ext: &Extents,
        at: u32,
        due: &Deadline,
    ) -> Result<u32, CardFail> {
        let mut us = 0u32;
        for (lba, n) in ext.transfers(at, READ_BLOCKS, READ_BLOCKS) {
            us = us.saturating_add(self.timed_read(store, lba, n, due)?);
        }
        Ok(us)
    }

    /// One multi-block read under its own `SD_IDLE_MS`, in µs. A failure
    /// re-inits the card, as `FatStore` does; a read past the cycle
    /// counter's wrap is `ClockWrap`.
    fn timed_read(
        &mut self,
        store: &SdStore,
        lba: u32,
        n: u32,
        due: &Deadline,
    ) -> Result<u32, CardFail> {
        due.check()?;
        let dev = store.device();
        let blocks = self
            .bufs
            .blocks
            .get_mut(..n as usize)
            .ok_or(StoreError::Io)?;
        let (t, ms) = (DWT::cycle_count(), now_ms());
        dev.start_op();
        dev.read(blocks, BlockIdx(lba)).map_err(|e| {
            let why = dev.classify(&e);
            dev.reinit();
            CardFail::Store(why)
        })?;
        let cycles = DWT::cycle_count().wrapping_sub(t);
        span_us(cycles, self.cpu_hz, now_ms().since(ms)).ok_or(CardFail::ClockWrap)
    }
}

fn throughput(blocks: u32, us: u64) -> Throughput {
    Throughput {
        bytes: blocks * Block::LEN_U32,
        us: u32::try_from(us).unwrap_or(u32::MAX),
    }
}

fn load(stats: Option<&mut Reader<AudioStats>>) -> Option<Load> {
    crate::audio_stats(stats).map(|a| Load {
        voices: a.voices,
        load_pct: a.load_avg,
    })
}

/// Both test files' runs, each written first if missing or short.
fn test_files(
    store: &mut SdStore,
    vol: VolumeId,
    due: &Deadline,
) -> Result<([Extents; FILES], Files), CardFail> {
    let mut ext = [Extents::new(); FILES];
    let mut files = Files::default();
    for (i, e) in ext.iter_mut().enumerate() {
        let name = test_file(i);
        *e = match whole_file(store, vol, name)? {
            Some(e) => e,
            None => {
                write_test_file(store, vol, name, FILE_AIMS[i], due)?;
                files.created += 1;
                whole_file(store, vol, name)?.ok_or(StoreError::Corrupt)?
            }
        };
        if !e.whole() {
            return Err(CardFail::Fragmented);
        }
        files.runs += e.seen();
    }
    let [a, b] = ext.map(|e| e.first().unwrap_or(0));
    files.apart_mib = a.abs_diff(b) / 2_048;
    Ok((ext, files))
}

/// `name`'s runs if it holds a whole test file; `None` if it is missing
/// or short.
fn whole_file(
    store: &mut SdStore,
    vol: VolumeId,
    name: FileName,
) -> Result<Option<Extents>, CardFail> {
    let mut ext = Extents::new();
    match store.runs(vol, name, &mut |at, n| ext.push(at, n)) {
        Ok(len) if len >= FILE_BYTES => Ok(Some(ext)),
        Ok(_) | Err(StoreError::NotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// A test file, aimed `aim` per mille into the card: only with room for it
/// and `ROOM_MARGIN_BYTES` after, and within the deadline. One that
/// doesn't complete is deleted.
fn write_test_file(
    store: &mut SdStore,
    vol: VolumeId,
    name: FileName,
    aim: u16,
    due: &Deadline,
) -> Result<(), CardFail> {
    if due.left_ms() < WRITE_MIN_MS {
        return Err(CardFail::Timeout);
    }
    if !store.has_room(vol, FILE_BYTES + ROOM_MARGIN_BYTES)? {
        return Err(CardFail::NoRoom);
    }
    store.aim(vol, aim)?;
    store.device().spi(|s| s.set_cap_ms(due.left_ms()));
    let wrote = store.write(vol, name, &mut |sink: &mut dyn ByteSink| {
        let mut chunk = [0u8; CHUNK];
        for i in 0..FILE_BYTES / CHUNK as u32 {
            chunk.fill(i as u8);
            sink.put(&chunk)?;
        }
        Ok(())
    });
    store.device().spi(|s| s.set_cap_ms(SD_OP_CAP_MS));
    match wrote {
        Ok(n) if n == FILE_BYTES => Ok(()),
        short => {
            // A failed write re-inited the card: mount it again first.
            let _ = store.mount().and_then(|v| store.delete(v, name));
            Err(match short {
                Err(StoreError::Timeout) => CardFail::SlowWrite,
                Err(StoreError::Full) => CardFail::NoRoom,
                Err(e) => CardFail::Store(e),
                Ok(_) => CardFail::Store(StoreError::Io),
            })
        }
    }
}

/// `card clean`: every file in `/CHIMERA/BENCH`, then the directory.
fn clean(store: &mut SdStore) -> Result<(), CardFail> {
    let vol = store.mount()?;
    loop {
        let mut names = [None; CLEAN_BATCH];
        let mut n = 0;
        match store.list(vol, Dir::Bench, &mut |f, _| {
            if let Some(slot) = names.get_mut(n) {
                *slot = Some(f);
                n += 1;
            }
        }) {
            Err(StoreError::NotFound) => return Ok(()),
            r => r?,
        }
        if n == 0 {
            break;
        }
        for f in names.into_iter().flatten() {
            store.delete(vol, f)?;
        }
    }
    match store.remove_dir(vol, Dir::Bench) {
        Ok(true) | Err(StoreError::NotFound) => Ok(()),
        Ok(false) => Err(CardFail::NotEmpty),
        Err(e) => Err(e.into()),
    }
}
