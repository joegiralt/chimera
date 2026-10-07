//! The console's `card` on the unit (bench builds): the SD driver's own
//! multi-block reads over two test files' runs, each read timed by the
//! cycle counter, from the UI loop while audio runs. Every read keeps
//! `SD_IDLE_MS` and the whole run `DEADLINE_MS`; audio, MIDI and the
//! watchdog's kicks run above it in interrupts throughout.

use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_core::boot::wait_until;
use chimera_core::console::{
    CardAnswer, CardArg, CardFail, CardInfo, CardKind, CardSpeed, DEADLINE_MS, Extents, FILE_AIMS,
    FILE_BYTES, FILES, Files, Latency, Load, MIN_APART_PER_MILLE, RANDOM_READS, READ_BLOCKS,
    ROOM_MARGIN_BYTES, SEQ_BLOCKS, SPAN_BLOCKS, STREAM_ROUNDS, STREAMS, Throughput,
    apart_per_mille, interleaved, random_reads, span_start, span_us,
};
use chimera_core::note_queue::{NoteEvent, NoteKind, NoteProducer};
use chimera_core::perf::load::AudioStats;
use chimera_core::triple::Reader;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_fat::Medium;
use chimera_hal::Ms;
use chimera_hal::store::{ByteSink, CHUNK, Dir, FileName, Store, StoreError, VolumeId};
use cortex_m::peripheral::DWT;
use embedded_sdmmc::embedded_sdmmc_types::sdcard::CardType;
use embedded_sdmmc::{Block, BlockDevice, BlockIdx};

use crate::controls::now_ms;
use crate::sd::{SD_OP_CAP_MS, SdStore};
use crate::shared::{BLOCKS, OVERRUNS};

/// The largest transfer, 16 KB.
const BUF_BLOCKS: usize = 32;
const _: () = assert!(READ_BLOCKS as usize <= BUF_BLOCKS);
/// Audio counts as running if it renders a block within this.
const LIVE_MS: u32 = 10;
/// Kept back from a test file's write for deleting it if it fails.
const DELETE_RESERVE_MS: u32 = 3_000;
/// A test file's write starts only with this long left of the deadline.
const WRITE_MIN_MS: u32 = DELETE_RESERVE_MS + 1_000;
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

    /// The next card operation's cap: what is left, less `reserve_ms`
    /// kept for what must follow it. `Timeout` if nothing is.
    fn arm(&self, store: &SdStore, reserve_ms: u32) -> Result<(), CardFail> {
        let left = self.left_ms().saturating_sub(reserve_ms);
        if left == 0 {
            return Err(CardFail::Timeout);
        }
        store.device().spi(|s| s.set_cap_ms(left));
        Ok(())
    }

    /// `f`, one card operation, under `arm(0)`: a card timeout once the
    /// deadline is spent is the deadline's.
    fn op<T>(
        &self,
        store: &mut SdStore,
        f: impl FnOnce(&mut SdStore) -> Result<T, StoreError>,
    ) -> Result<T, CardFail> {
        self.arm(store, 0)?;
        f(store).map_err(|e| match e {
            StoreError::Timeout if self.left_ms() == 0 => CardFail::Timeout,
            e => CardFail::Store(e),
        })
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
        let due = Deadline(now_ms());
        let answer = match arg {
            CardArg::Clean => CardAnswer::Cleaned(clean(store, &due)),
            CardArg::Run(seed) => {
                let mut s = CardSpeed {
                    seed: seed.unwrap_or_else(DWT::cycle_count),
                    ..CardSpeed::default()
                };
                let fail = self.measure(&mut s, store, stats, &due).err();
                CardAnswer::Measured(s, fail)
            }
        };
        // Every operation since ran under the deadline's cap.
        store.device().spi(|s| s.set_cap_ms(SD_OP_CAP_MS));
        answer
    }

    fn measure(
        &mut self,
        s: &mut CardSpeed,
        store: &mut SdStore,
        stats: Option<&mut Reader<AudioStats>>,
        due: &Deadline,
    ) -> Result<(), CardFail> {
        let seen = BLOCKS.load(Ordering::Relaxed);
        let live = wait_until(LIVE_MS * (self.cpu_hz / 1_000), DWT::cycle_count, || {
            BLOCKS.load(Ordering::Relaxed) != seen
        });
        if !live {
            return Err(CardFail::NoAudio);
        }
        let overruns = OVERRUNS.load(Ordering::Relaxed);
        let r = self.phases(s, store, stats, due);
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
        let vol = due.op(store, |s| s.mount())?;
        due.op(store, |s| s.make_dir(vol, Dir::Chimera))?;
        due.op(store, |s| s.make_dir(vol, Dir::Bench))?;
        let bytes = due.op(store, |s| {
            let dev = s.device();
            dev.start_op();
            dev.num_bytes().map_err(|e| dev.classify(&e))
        })?;
        let block_addressed = store.device().get_card_type() == Some(CardType::SdhcSdxc);
        let (ext, files, free) = test_files(store, vol, due)?;
        s.card = Some(CardInfo {
            kind: CardKind::of(block_addressed, bytes),
            bytes,
            free_bytes: free,
        });
        let [a, b] = ext.map(|e| e.first().unwrap_or(0));
        let files = Files {
            apart_mib: a.abs_diff(b) / 2_048,
            apart_per_mille: apart_per_mille(a, b, bytes),
            ..files
        };
        s.files = Some(files);
        if u64::from(files.apart_per_mille) < MIN_APART_PER_MILLE {
            return Err(CardFail::TooClose);
        }
        for ((i, &n), out) in (0..).zip(&SEQ_BLOCKS).zip(&mut s.seq) {
            *out = Some(self.sequential(store, &ext[0], span_start(i), n, due)?);
        }
        self.chord(NoteKind::On(Velocity::DEFAULT));
        crate::clocks::delay_us(self.cpu_hz, SETTLE_US);
        let mut loaded = || -> Result<(), CardFail> {
            s.random = Some(self.random(store, &ext, s.seed, due)?);
            s.load = load(stats.as_deref_mut());
            let (mean, rounds) = self.interleaved(store, &ext, due)?;
            (s.interleaved, s.rounds) = (Some(mean), Some(rounds));
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

    /// TRON's pattern, `interleaved()`: its mean rate, and each round's
    /// time (every stream refilled once).
    fn interleaved(
        &mut self,
        store: &SdStore,
        ext: &[Extents; FILES],
        due: &Deadline,
    ) -> Result<(Throughput, Latency), CardFail> {
        let (mut us, mut blocks, mut round) = (0u64, 0u32, 0u32);
        for (i, (file, at)) in (1..).zip(interleaved()) {
            let t = self.read_4k(store, &ext[file], at, due)?;
            (us, blocks, round) = (us + u64::from(t), blocks + READ_BLOCKS, round + t);
            if i % STREAMS == 0 {
                if let Some(slot) = self.bufs.lat.get_mut((i / STREAMS - 1) as usize) {
                    *slot = round;
                }
                round = 0;
            }
        }
        let rounds = self
            .bufs
            .lat
            .get_mut(..STREAM_ROUNDS as usize)
            .unwrap_or(&mut []);
        Ok((
            throughput(blocks, us),
            Latency::of(rounds).unwrap_or_default(),
        ))
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
        due.arm(store, 0)?;
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

/// Both test files' runs, each written first if missing or short, and
/// the card's free bytes after: counted from the FAT only when a file is
/// written, `None` when both were there.
fn test_files(
    store: &mut SdStore,
    vol: VolumeId,
    due: &Deadline,
) -> Result<([Extents; FILES], Files, Option<u64>), CardFail> {
    let mut found = [None; FILES];
    for (i, f) in found.iter_mut().enumerate() {
        *f = whole_file(store, vol, test_file(i), due)?;
    }
    let mut free = None;
    if found.iter().any(Option::is_none) {
        // The whole FAT read: a few s on a 32 GB card.
        free = Some(due.op(store, |s| s.free_bytes(vol))?);
    }
    let mut ext = [Extents::new(); FILES];
    let mut files = Files::default();
    for (i, (e, f)) in ext.iter_mut().zip(found).enumerate() {
        let name = test_file(i);
        *e = match f {
            Some(e) => e,
            None => {
                let room = free.unwrap_or(0);
                write_test_file(store, vol, name, FILE_AIMS[i], room, due)?;
                free = Some(room.saturating_sub(u64::from(FILE_BYTES)));
                files.created += 1;
                whole_file(store, vol, name, due)?.ok_or(StoreError::Corrupt)?
            }
        };
        if !e.whole() {
            return Err(CardFail::Fragmented);
        }
        files.runs += e.seen();
    }
    Ok((ext, files, free))
}

/// `name`'s runs if it holds a whole test file; `None` if it is missing
/// or short.
fn whole_file(
    store: &mut SdStore,
    vol: VolumeId,
    name: FileName,
    due: &Deadline,
) -> Result<Option<Extents>, CardFail> {
    let mut ext = Extents::new();
    let len = due.op(store, |s| {
        match s.runs(vol, name, &mut |at, n| ext.push(at, n)) {
            Err(StoreError::NotFound) => Ok(0),
            r => r,
        }
    })?;
    Ok((len >= FILE_BYTES).then_some(ext))
}

/// A test file, aimed `aim` per mille into the card: only with `free`
/// holding it and `ROOM_MARGIN_BYTES` after, and within the deadline. One
/// that doesn't complete is deleted.
fn write_test_file(
    store: &mut SdStore,
    vol: VolumeId,
    name: FileName,
    aim: u16,
    free: u64,
    due: &Deadline,
) -> Result<(), CardFail> {
    if due.left_ms() < WRITE_MIN_MS {
        return Err(CardFail::Timeout);
    }
    if free < u64::from(FILE_BYTES + ROOM_MARGIN_BYTES) {
        return Err(CardFail::NoRoom);
    }
    due.op(store, |s| s.aim(vol, aim))?;
    // The write's cap keeps `DELETE_RESERVE_MS` back: a write that runs
    // out of time is always deleted.
    due.arm(store, DELETE_RESERVE_MS)?;
    let wrote = store.write(vol, name, &mut |sink: &mut dyn ByteSink| {
        let mut chunk = [0u8; CHUNK];
        for i in 0..FILE_BYTES / CHUNK as u32 {
            chunk.fill(i as u8);
            sink.put(&chunk)?;
        }
        Ok(())
    });
    match wrote {
        Ok(n) if n == FILE_BYTES => Ok(()),
        short => {
            // A failed write re-inited the card: mount it again first.
            let _ = due.op(store, |s| s.mount().and_then(|v| s.delete(v, name)));
            Err(match short {
                Err(StoreError::Timeout) => CardFail::SlowWrite,
                Err(StoreError::Full) => CardFail::NoRoom,
                Err(e) => CardFail::Store(e),
                Ok(_) => CardFail::Store(StoreError::Io),
            })
        }
    }
}

/// `card clean`: every file in `/CHIMERA/BENCH`, then the directory,
/// each operation under the deadline.
fn clean(store: &mut SdStore, due: &Deadline) -> Result<(), CardFail> {
    let vol = due.op(store, |s| s.mount())?;
    loop {
        let mut names = [None; CLEAN_BATCH];
        let mut n = 0;
        let listed = due.op(store, |s| {
            s.list(vol, Dir::Bench, &mut |f, _| {
                if let Some(slot) = names.get_mut(n) {
                    *slot = Some(f);
                    n += 1;
                }
            })
        });
        match listed {
            Err(CardFail::Store(StoreError::NotFound)) => return Ok(()),
            r => r?,
        }
        if n == 0 {
            break;
        }
        for f in names.into_iter().flatten() {
            due.op(store, |s| s.delete(vol, f))?;
        }
    }
    match due.op(store, |s| s.remove_dir(vol, Dir::Bench)) {
        Ok(true) | Err(CardFail::Store(StoreError::NotFound)) => Ok(()),
        Ok(false) => Err(CardFail::NotEmpty),
        Err(e) => Err(e),
    }
}
