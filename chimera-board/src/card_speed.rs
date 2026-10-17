//! The console's `card` on the unit (bench builds): multi-block reads over
//! two test files' runs, each posted to the card IRQ (`BenchPort`) and
//! timed there by the cycle counter, from the UI loop while audio runs.
//! Every read keeps `SD_IDLE_MS` and the whole run `DEADLINE_MS`; audio,
//! MIDI and the watchdog's kicks run above it in interrupts throughout.
//!
//! `card dma` (B2–B10) runs on the streaming driver itself: each verb
//! starts a run that `CardBench::tick` moves on from every UI pass, timing
//! and moving bytes only; the console's `card` math holds the rest. All
//! of it only with bank 2: the other bench sets have no FLASH for it.

use core::hint::black_box;
use core::mem::MaybeUninit;
use core::ops::ControlFlow;
use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_core::boot::wait_until;
use chimera_core::console::{
    CardAnswer, CardArg, CardFail, CardInfo, CardKind, CardSpeed, DEADLINE_MS, DmaAnswer, DmaOwn,
    DmaRefusal, DmaReport, DmaVerb, Extents, FILE_AIMS, FILE_BLOCKS, FILE_BYTES, FILES, Files,
    Histogram, Inject, IrqCounts, Latency, Load, MIN_APART_PER_MILLE, Percentile, RANDOM_READS,
    READ_BLOCKS, ROOM_MARGIN_BYTES, SEQ_BLOCKS, SPAN_BLOCKS, STREAM_ROUNDS, STREAMS, StreamLines,
    Throughput, apart_per_mille, fit_line, interleaved, random_reads, span_start,
};
use chimera_core::dsp::tape_engine::read::Shape;
use chimera_core::hw::MAX_VOICES;
use chimera_core::note_queue::{NoteEvent, NoteKind, NoteProducer};
use chimera_core::perf::load::AudioStats;
use chimera_core::storage::{
    HEADER_LEN, RecordTag, SOURCE_HEAD_LEN, SourceHead, SourceLayout, source_name,
};
use chimera_core::stream::bench::{
    ArmError, BENCH, BedCheck, BedTable, Listen, MAX_BED_CHUNKS, bed_id,
};
use chimera_core::stream::layout::{ChunkIdx, StreamFile};
use chimera_core::stream::registry::StreamRegistry;
use chimera_core::stream::units::{Ratio, SourceRate};
use chimera_core::triple::Reader;
use chimera_core::voice_alloc::VoiceIdx;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_fat::Medium;
use chimera_fat::sd_addr::AddrMode;
use chimera_fat::sd_proto::crc16;
use chimera_hal::Ms;
use chimera_hal::store::{
    ByteSink, CHUNK, Dir, FileName, ReadSink, RunsError, Store, StoreError, VolumeId,
};
use chimera_hal::units::{BlockCount, Bytes, FileBlock, Lba, Micros, Millis, StreamCount};
use cortex_m::peripheral::DWT;
use embedded_sdmmc::{Block, BlockDevice, BlockIdx};

use crate::card_dma::{self, BenchPort, FILL_RING, STATS, bench_port};
use crate::controls::now_ms;
use crate::sd::SdStore;
use crate::shared::{BLOCKS, OVERRUNS, registry};

const READ_RUN: BlockCount = match BlockCount::new(READ_BLOCKS) {
    Some(n) => n,
    None => panic!("READ_BLOCKS is 0"),
};
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

/// The latency samples and the DMA benches' buffers: a static, not the
/// stack. All zero, so `.bss`.
struct Bufs {
    lat: [u32; RANDOM_READS],
    /// A B2 set's reads, for its p99.
    set: Histogram<SET_BUCKETS>,
    /// `BEDS.CHK`, read once a boot.
    table: [u8; TABLE_MAX],
    /// B5's 4 KB.
    write: [Block; WRITE_BLOCKS],
}

static mut BUFS: Bufs = Bufs {
    lat: [0; RANDOM_READS],
    set: Histogram::new(),
    table: [0; TABLE_MAX],
    write: [const {
        Block {
            contents: [0; Block::LEN],
        }
    }; WRITE_BLOCKS],
};
static BUFS_TAKEN: AtomicBool = AtomicBool::new(false);
// 10,004 B, NOLOAD in D2 beside the UI: built in place by `take`.
#[unsafe(link_section = ".ram_d2")]
static mut HIST: MaybeUninit<Histogram<HIST_BUCKETS>> = MaybeUninit::uninit();
static mut DMA: MaybeUninit<Dma> = MaybeUninit::uninit();

pub struct CardBench {
    bufs: &'static mut Bufs,
    /// The run's histogram: every verb's timed operation.
    hist: &'static mut Histogram<HIST_BUCKETS>,
    dma: &'static mut Dma,
    port: BenchPort,
    notes: NoteProducer<'static>,
    cpu_hz: u32,
}

/// The one `CardBench`, over the `CARD` note source and the card IRQ's
/// bench port.
pub fn take(notes: NoteProducer<'static>, cpu_hz: u32) -> Option<CardBench> {
    if BUFS_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    let port = bench_port()?;
    // SAFETY: the flag lets exactly one caller past, so these are the only
    // references to `BUFS`, `HIST` and `DMA` ever made.
    let (bufs, hist, dma) = unsafe {
        (
            &mut *addr_of_mut!(BUFS),
            Histogram::init_in_place(&mut *addr_of_mut!(HIST)),
            (*addr_of_mut!(DMA)).write(Dma::new()),
        )
    };
    Some(CardBench {
        bufs,
        hist,
        dma,
        port,
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
        store.device().cap(Some(left));
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
            CardArg::Dma(verb) => CardAnswer::Dma(self.dma(verb, store, stats)),
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
        store.device().cap(None);
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
            s.device().with_device(|dev| {
                dev.start_op();
                dev.num_bytes().map_err(|e| dev.classify(&e))
            })?
        })?;
        let block_addressed = store.device().addr_mode() == Some(AddrMode::Block);
        let (ext, files, free) = test_files(store, vol, due)?;
        s.card = Some(CardInfo {
            kind: CardKind::of(block_addressed, bytes),
            bytes,
            free_bytes: free,
        });
        let [a, b] = ext.map(|e| e.first().get());
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
            *out = Some(self.sequential(&ext[0], span_start(i), n, due)?);
        }
        self.chord(NoteKind::On(Velocity::DEFAULT));
        crate::clocks::delay_us(self.cpu_hz, SETTLE_US);
        let mut loaded = || -> Result<(), CardFail> {
            s.random = Some(self.random(&ext, s.seed, due)?);
            s.load = load(stats.as_deref_mut());
            let (mean, rounds) = self.interleaved(&ext, due)?;
            (s.interleaved, s.rounds) = (Some(mean), Some(rounds));
            s.load = load(stats.as_deref_mut()).or(s.load);
            Ok(())
        };
        let r = loaded();
        self.chord(NoteKind::Off);
        r
    }

    fn chord(&mut self, kind: NoteKind) {
        chord(&mut self.notes, kind);
    }

    /// `SPAN_BLOCKS` from file block `from`, `max` blocks a transfer.
    fn sequential(
        &mut self,
        ext: &Extents,
        from: u32,
        max: u32,
        due: &Deadline,
    ) -> Result<Throughput, CardFail> {
        let mut us = 0u64;
        let max = BlockCount::new(max).ok_or(StoreError::Io)?;
        for (lba, n) in ext.transfers(FileBlock::new(from), SPAN_BLOCKS, max) {
            us += u64::from(self.read(lba, n, due)?);
        }
        Ok(throughput(SPAN_BLOCKS, us))
    }

    /// `RANDOM_READS` timed reads of 4 KB across both files.
    fn random(
        &mut self,
        ext: &[Extents; FILES],
        seed: u32,
        due: &Deadline,
    ) -> Result<Latency, CardFail> {
        for (i, (file, at)) in random_reads(seed).enumerate() {
            let us = self.read_4k(&ext[file], at, due)?;
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
        ext: &[Extents; FILES],
        due: &Deadline,
    ) -> Result<(Throughput, Latency), CardFail> {
        let (mut us, mut blocks, mut round) = (0u64, 0u32, 0u32);
        for (i, (file, at)) in (1..).zip(interleaved()) {
            let t = self.read_4k(&ext[file], at, due)?;
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
    fn read_4k(&mut self, ext: &Extents, at: u32, due: &Deadline) -> Result<u32, CardFail> {
        let mut us = 0u32;
        for (lba, n) in ext.transfers(FileBlock::new(at), READ_BLOCKS, READ_RUN) {
            us = us.saturating_add(self.read(lba, n, due)?);
        }
        Ok(us)
    }

    /// One multi-block read by the card IRQ, under its own `SD_IDLE_MS`,
    /// in µs from CMD18 to the stop. The IRQ checks every block's CRC and
    /// keeps none; a failure leaves the card as the IRQ's abort left it.
    fn read(&mut self, lba: Lba, n: BlockCount, due: &Deadline) -> Result<u32, CardFail> {
        if due.left_ms() == 0 {
            return Err(CardFail::Timeout);
        }
        let us = self.port.read(lba, n).map_err(StoreError::from)?;
        Ok(us.get())
    }
}

fn chord(notes: &mut NoteProducer<'static>, kind: NoteKind) {
    for n in CHORD.into_iter().filter_map(MidiNote::new) {
        notes.push(NoteEvent {
            channel: MidiChannel::clamped(0),
            note: n,
            kind,
        });
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
    let mut files = Files::default();
    for (i, f) in found.iter_mut().enumerate() {
        let name = test_file(i);
        if f.is_none() {
            let room = free.unwrap_or(0);
            write_test_file(store, vol, name, FILE_AIMS[i], room, due)?;
            free = Some(room.saturating_sub(u64::from(FILE_BYTES)));
            files.created += 1;
            *f = Some(whole_file(store, vol, name, due)?.ok_or(StoreError::Corrupt)?);
        }
        files.runs += f.map_or(0, |e| e.runs());
    }
    let [Some(a), Some(b)] = found else {
        return Err(StoreError::Corrupt.into());
    };
    Ok(([a, b], files, free))
}

/// `name`'s runs if it holds a whole test file; `None` if it is missing
/// or short.
fn whole_file(
    store: &mut SdStore,
    vol: VolumeId,
    name: FileName,
    due: &Deadline,
) -> Result<Option<Extents>, CardFail> {
    let found = due.op(store, |s| match Store::runs(s, vol, name) {
        Ok(r) => Ok(Some(Ok(r))),
        Err(RunsError::Store(StoreError::NotFound)) => Ok(None),
        Err(RunsError::Store(e)) => Err(e),
        Err(RunsError::Fragmented(_)) => Ok(Some(Err(CardFail::Fragmented))),
    })?;
    match found {
        None => Ok(None),
        Some(Err(f)) => Err(f),
        Some(Ok(r)) => Ok((r.len.get() >= FILE_BYTES).then_some(r.ext)),
    }
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

/// B2's sizes, in blocks: 1, 2, 4, 8 and 16 KB.
const B2_BLOCKS: [u32; 5] = [2, 4, 8, 16, 32];
/// B2's sets: each size idle, then each with the chord.
const B2_SETS: usize = 2 * B2_BLOCKS.len();
const B2_READS: u32 = 20_000;
const BURSTS: u32 = 1_000;
/// B7's reads each way.
const BACK_READS: u32 = 20_000;
/// B4's length; B6, B8 and B10 run as long.
const SUSTAIN_MS: u32 = 600_000;
const STALL_MS: u32 = 1_800_000;
const INJECT_MS: u32 = 120_000;
const INJECTIONS: u32 = 100;
const INJECT_EVERY_MS: u32 = 1_000;
/// B9's UI read, this long after its injection.
const INJECT_READ_MS: u32 = 10;
const WAKE_EVERY_MS: u32 = 50;
const WRITE_EVERY_MS: u32 = 1_000;
const SAVE_EVERY_MS: u32 = 60_000;
/// A tick's reads stop past this, so the UI loop runs between them.
const TICK_MS: u32 = 5;
/// B3's heads prime before its first burst.
const PRIME_MS: u32 = 1_000;
/// A burst not landed in this is recorded past the histogram.
const BURST_GIVE_UP_MS: u32 = 1_000;
/// B3 holds the heads all but still, so only its bursts read.
const STILL: Ratio = match Ratio::from_milli(1) {
    Some(r) => r,
    None => panic!(),
};
/// B3's burst: a read for every stream.
const BURST_READS: u32 = StreamCount::MAX.get() as u32;
const B9_STREAMS: StreamCount = match StreamCount::new(2) {
    Some(n) => n,
    None => panic!(),
};
const HIST_BUCKETS: usize = 2_500;
/// A B2 set's p99 past 100 ms drops its point from the fit.
const SET_BUCKETS: usize = 1_000;
/// `BEDS.CHK`'s cap, as `put`'s.
const TABLE_MAX: usize = 9 * 1024;
const _: () = assert!(5 + 8 * (6 + 4 * MAX_BED_CHUNKS) <= TABLE_MAX);
/// A source's head record: after the file's header, its tag and length.
const HEAD_AT: usize = HEADER_LEN + 4;
const HEAD_END: usize = HEAD_AT + SOURCE_HEAD_LEN;
const WRITE_BLOCKS: usize = 8;
const FOUR_K: BlockCount = match BlockCount::new(WRITE_BLOCKS as u32) {
    Some(n) => n,
    None => panic!(),
};
const _: () = assert!(FILE_BLOCKS.is_multiple_of(WRITE_BLOCKS as u32));

/// What the UI loop does for a run this pass.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ask {
    /// B6: every row redrawn.
    pub redraw: bool,
    /// B6: the project saved, then `saved`.
    pub save: bool,
}

/// The card IRQ's and the audio's counters at a run's start.
#[derive(Clone, Copy)]
struct Counts {
    overruns: u32,
    aborts: u32,
    stop_errors: u32,
    faults: u32,
    entries: u32,
    reads: u32,
    bursts: u32,
}

impl Counts {
    fn now() -> Counts {
        let get = |a: &core::sync::atomic::AtomicU32| a.load(Ordering::Relaxed);
        Counts {
            overruns: get(&OVERRUNS),
            aborts: get(&STATS.aborts),
            stop_errors: get(&STATS.stop_errors),
            faults: get(&STATS.faults),
            entries: get(&STATS.entries),
            reads: get(&STATS.reads),
            bursts: get(&STATS.bursts),
        }
    }

    /// A run's start: the counts, the bursts' max restarted.
    fn start() -> Counts {
        STATS.bursts_max.store(0, Ordering::Relaxed);
        Counts::now()
    }

    fn since(self, then: Counts) -> Counts {
        Counts {
            overruns: self.overruns.wrapping_sub(then.overruns),
            aborts: self.aborts.wrapping_sub(then.aborts),
            stop_errors: self.stop_errors.wrapping_sub(then.stop_errors),
            faults: self.faults.wrapping_sub(then.faults),
            entries: self.entries.wrapping_sub(then.entries),
            reads: self.reads.wrapping_sub(then.reads),
            bursts: self.bursts.wrapping_sub(then.bursts),
        }
    }
}

/// What a streaming run does besides its streams.
enum Side {
    /// B4, B8.
    Plain,
    /// B5: `writes` so far, into `Dma::stall`.
    Stall { writes: u32, errors: u32 },
    /// B6.
    Ui {
        last: Ms,
        window: Ms,
        passes: u32,
        fps_min: u32,
        frozen_ms: u32,
        saves: u32,
        save_ok: bool,
    },
    /// B9 with a fault, B10 without: `done` injections or reads asked.
    Reads {
        fault: Option<Inject>,
        done: u32,
        read_at: Option<Ms>,
        reads: u32,
        ok: u32,
    },
}

enum Work {
    /// B2: `done` reads of set `set`; each set's p99 a point.
    Chunk {
        set: usize,
        done: u32,
        points: [Option<(Bytes, Micros)>; B2_SETS],
        rng: u64,
    },
    Burst {
        done: u32,
    },
    /// B7: backward first, then forward; µs summed each way.
    Back {
        done: u32,
        at: (usize, u32),
        sums: [u64; 2],
        rng: u64,
    },
    /// `seen`: the fills drained into the histogram; `lost`: those the
    /// ring had moved past.
    Streams {
        n: StreamCount,
        ms: u32,
        seen: u32,
        lost: u32,
        side: Side,
    },
}

struct Run {
    began: Ms,
    card: (CardKind, u32),
    counts: Counts,
    work: Work,
}

/// `card dma`'s state: a static.
struct Dma {
    run: Option<Run>,
    last: Option<DmaReport>,
    /// The beds' volume, bed 0 for the UI reads, and the beds' rate.
    vol: Option<VolumeId>,
    bed0: Option<FileName>,
    rate: Option<SourceRate>,
    /// B5's test file's runs.
    stall: Option<Extents>,
    /// Each bed's runs on the card.
    runs: [u32; MAX_VOICES],
}

impl Dma {
    const fn new() -> Dma {
        Dma {
            run: None,
            last: None,
            vol: None,
            bed0: None,
            rate: None,
            stall: None,
            runs: [0; MAX_VOICES],
        }
    }
}

/// A cycle-counter span in µs.
fn cycles_us(cpu_hz: u32, cycles: u32) -> Micros {
    Micros::new(cycles / (cpu_hz / 1_000_000).max(1))
}

fn xorshift(state: &mut u64) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 32) as u32
}

/// The beds' files, once set: the registry's bed rows.
fn loaded_beds() -> impl Iterator<Item = &'static StreamFile> {
    let reg = card_dma::beds().and(registry());
    VoiceIdx::ALL
        .into_iter()
        .filter_map(move |v| reg?.get(bed_id(v)))
}

/// Chunk `c`'s first device block in loaded bed `k`.
fn chunk_lba(k: usize, c: u32) -> Option<Lba> {
    let bed = loaded_beds().nth(k)?;
    Some(bed.chunk_read(ChunkIdx::new(c)?)?.first()?.lba)
}

/// A random chunk's first block, of a random bed.
fn somewhere(rng: &mut u64) -> Option<Lba> {
    let k = xorshift(rng) as usize % loaded_beds().count().max(1);
    let last = loaded_beds().nth(k)?.last_chunk().get();
    chunk_lba(k, xorshift(rng) % (last + 1))
}

impl CardBench {
    /// A `card dma` verb's answer, at once.
    fn dma(
        &mut self,
        verb: DmaVerb,
        store: &mut SdStore,
        stats: Option<&mut Reader<AudioStats>>,
    ) -> DmaAnswer {
        match verb {
            DmaVerb::Report => match (&self.dma.run, &self.dma.last) {
                (Some(r), _) => DmaAnswer::Running(r.pct(now_ms())),
                (None, Some(r)) => DmaAnswer::Report(*r),
                (None, None) => DmaAnswer::Refused(DmaRefusal::NoRun),
            },
            DmaVerb::Stop => {
                self.finish(stats);
                DmaAnswer::Stopped
            }
            _ if self.dma.run.is_some() => DmaAnswer::Refused(DmaRefusal::Busy),
            _ => match self.start(verb, store) {
                Ok(()) => DmaAnswer::Started(verb),
                Err(e) => DmaAnswer::Refused(e),
            },
        }
    }

    fn start(&mut self, verb: DmaVerb, store: &mut SdStore) -> Result<(), DmaRefusal> {
        let card = |e: StoreError| DmaRefusal::Card(CardFail::Store(e));
        let vol = store.mount().map_err(card)?;
        let bytes = store
            .device()
            .with_device(|dev| {
                dev.start_op();
                dev.num_bytes().map_err(|e| dev.classify(&e))
            })
            .map_err(|b| card(b.into()))?
            .map_err(card)?;
        let block_addressed = store.device().addr_mode() == Some(AddrMode::Block);
        self.load_beds(store, vol)?;
        let (Some(reg), Some(rate)) = (card_dma::beds().and(registry()), self.dma.rate) else {
            return Err(DmaRefusal::Beds);
        };
        let arm = |n, speed, listen| {
            BENCH
                .arm(reg, rate, n, speed, listen)
                .map_err(|ArmError::Beds| DmaRefusal::Beds)
        };
        let streams = |n, ms, side| Work::Streams {
            n,
            ms,
            seen: card_dma::fills(),
            lost: 0,
            side,
        };
        let now = now_ms();
        let work = match verb {
            DmaVerb::Chunk => {
                BENCH.disarm();
                Work::Chunk {
                    set: 0,
                    done: 0,
                    points: [None; B2_SETS],
                    rng: 1 << 32 | u64::from(DWT::cycle_count()),
                }
            }
            DmaVerb::Burst => {
                arm(StreamCount::MAX, STILL, Listen::Off)?;
                Work::Burst { done: 0 }
            }
            DmaVerb::Back => {
                BENCH.disarm();
                Work::Back {
                    done: 0,
                    at: (0, loaded_beds().next().map_or(0, |b| b.last_chunk().get())),
                    sums: [0; 2],
                    rng: 1 << 32 | u64::from(DWT::cycle_count()),
                }
            }
            DmaVerb::Sustain(n) => {
                arm(n, Ratio::ONE, Listen::Off)?;
                streams(n, SUSTAIN_MS, Side::Plain)
            }
            DmaVerb::Listen(n) => {
                arm(n, Ratio::ONE, Listen::Stream0)?;
                streams(n, SUSTAIN_MS, Side::Plain)
            }
            DmaVerb::Stall(n) => {
                let due = Deadline(now);
                let ext = test_files(store, vol, &due).map(|(ext, ..)| ext[0]);
                store.device().cap(None);
                self.dma.stall = Some(ext.map_err(DmaRefusal::Card)?);
                arm(n, Ratio::ONE, Listen::Off)?;
                streams(
                    n,
                    STALL_MS,
                    Side::Stall {
                        writes: 0,
                        errors: 0,
                    },
                )
            }
            DmaVerb::Ui(n) => {
                arm(n, Ratio::ONE, Listen::Off)?;
                let side = Side::Ui {
                    last: now,
                    window: now,
                    passes: 0,
                    fps_min: u32::MAX,
                    frozen_ms: 0,
                    saves: 0,
                    save_ok: true,
                };
                streams(n, SUSTAIN_MS, side)
            }
            DmaVerb::Wake(n) => {
                arm(n, Ratio::ONE, Listen::Off)?;
                streams(n, SUSTAIN_MS, reads(None))
            }
            DmaVerb::Inject(k) => {
                arm(B9_STREAMS, Ratio::ONE, Listen::Off)?;
                streams(B9_STREAMS, INJECT_MS, reads(Some(k)))
            }
            DmaVerb::Report | DmaVerb::Stop => return Err(DmaRefusal::NoRun),
        };
        self.hist.clear();
        self.bufs.set.clear();
        if !matches!(work, Work::Chunk { .. }) {
            self.chord(NoteKind::On(Velocity::DEFAULT));
        }
        self.dma.run = Some(Run {
            began: now,
            card: (
                CardKind::of(block_addressed, bytes),
                u32::try_from(bytes / 1_000_000).unwrap_or(u32::MAX),
            ),
            counts: Counts::start(),
            work,
        });
        Ok(())
    }

    /// `BEDS.CHK` and every bed it lists, once a boot: bed k built in
    /// place from its source's runs and head.
    fn load_beds(&mut self, store: &mut SdStore, vol: VolumeId) -> Result<(), DmaRefusal> {
        if card_dma::beds().is_some() {
            return Ok(());
        }
        let name = FileName::new(Dir::Bench, b"BEDS", b"CHK").ok_or(DmaRefusal::Beds)?;
        let mut sink = Fill {
            buf: &mut self.bufs.table,
            n: 0,
            over: false,
        };
        let read = store.read(vol, name, &mut sink);
        let (n, over) = (sink.n, sink.over);
        if read.is_err() || over {
            return Err(DmaRefusal::Beds);
        }
        let table = BedTable::parse(&self.bufs.table[..n]).map_err(|_| DmaRefusal::Beds)?;
        let reg = registry().ok_or(DmaRefusal::Beds)?;
        let mut first = None;
        let mut runs = [0; MAX_VOICES];
        let ok = card_dma::fill_beds(|beds| {
            table.count() > 0
                && beds
                    .slots()
                    .iter_mut()
                    .enumerate()
                    .take(table.count())
                    .zip(&mut runs)
                    .all(|((k, slot), runs)| {
                        let Some((name, rate, n)) = place(store, vol, &table, k, reg, slot) else {
                            return false;
                        };
                        *runs = n;
                        if k == 0 {
                            first = Some((name, rate));
                        }
                        true
                    })
        });
        if !ok {
            for v in VoiceIdx::ALL {
                // SAFETY: the beds were never Ready, so the bench was never
                // armed: no want names a bed, and neither the audio nor the
                // IRQ (`card_dma::bed`) has read a bed's row.
                let _ = unsafe { reg.clear(bed_id(v)) };
            }
            return Err(DmaRefusal::Beds);
        }
        let (bed0, rate) = first.ok_or(DmaRefusal::Beds)?;
        (self.dma.vol, self.dma.bed0, self.dma.rate) = (Some(vol), Some(bed0), Some(rate));
        self.dma.runs = runs;
        Ok(())
    }

    /// B6's save, done: one that failed fails the run's `save_ok`.
    pub fn saved(&mut self, ok: bool) {
        if let Some(Run {
            work:
                Work::Streams {
                    side: Side::Ui { save_ok, .. },
                    ..
                },
            ..
        }) = &mut self.dma.run
        {
            *save_ok &= ok;
        }
    }

    /// The UI loop's, every pass: the run moved on, ended when done.
    pub fn tick(
        &mut self,
        now: Ms,
        store: &mut SdStore,
        stats: Option<&mut Reader<AudioStats>>,
    ) -> Ask {
        let mut ask = Ask::default();
        let Dma {
            run,
            vol,
            bed0,
            stall,
            ..
        } = &mut *self.dma;
        let Some(run) = run else {
            return ask;
        };
        let since = now.since(run.began);
        let done = match &mut run.work {
            Work::Chunk {
                set,
                done,
                points,
                rng,
            } => {
                while *set < B2_SETS && now_ms().since(now) < TICK_MS {
                    let blocks = B2_BLOCKS[*set % B2_BLOCKS.len()];
                    if *set == B2_BLOCKS.len() && *done == 0 {
                        chord(&mut self.notes, NoteKind::On(Velocity::DEFAULT));
                        crate::clocks::delay_us(self.cpu_hz, SETTLE_US);
                    }
                    let n = BlockCount::new(blocks).unwrap_or(BlockCount::ONE);
                    if let Some(us) = somewhere(rng).and_then(|lba| self.port.read(lba, n).ok()) {
                        self.hist.record(us);
                        self.bufs.set.record(us);
                    }
                    *done += 1;
                    if *done == B2_READS {
                        points[*set] = match self.bufs.set.percentile(990) {
                            Percentile::Within(us) => Some((n.bytes(), us)),
                            Percentile::Over => None,
                        };
                        self.bufs.set.clear();
                        (*set, *done) = (*set + 1, 0);
                    }
                }
                *set == B2_SETS
            }
            Work::Burst { done } => {
                if since >= PRIME_MS {
                    let f0 = card_dma::fills();
                    let t0 = DWT::cycle_count();
                    BENCH.queue_burst();
                    let landed = loop {
                        if card_dma::fills().wrapping_sub(f0) >= BURST_READS {
                            break true;
                        }
                        let t = now_ms();
                        card_dma::watch(t);
                        if t.since(now) > BURST_GIVE_UP_MS {
                            break false;
                        }
                    };
                    let took = DWT::cycle_count().wrapping_sub(t0);
                    self.hist.record(if landed {
                        cycles_us(self.cpu_hz, took)
                    } else {
                        Micros::new(u32::MAX)
                    });
                    *done += 1;
                }
                *done >= BURSTS
            }
            Work::Back {
                done,
                at,
                sums,
                rng,
            } => {
                while *done < 2 * BACK_READS && now_ms().since(now) < TICK_MS {
                    let back = *done < BACK_READS;
                    let lba = if back {
                        let lba = chunk_lba(at.0, at.1);
                        *at = match at.1.checked_sub(1) {
                            Some(c) => (at.0, c),
                            None => {
                                let k = (at.0 + 1) % loaded_beds().count().max(1);
                                let last = loaded_beds().nth(k).map_or(0, |b| b.last_chunk().get());
                                (k, last)
                            }
                        };
                        lba
                    } else {
                        somewhere(rng)
                    };
                    if let Some(us) = lba.and_then(|lba| self.port.read(lba, FOUR_K).ok()) {
                        if back {
                            self.hist.record(us);
                        }
                        sums[usize::from(!back)] += u64::from(us.get());
                    }
                    *done += 1;
                }
                *done >= 2 * BACK_READS
            }
            Work::Streams {
                ms,
                seen,
                lost,
                side,
                ..
            } => {
                let head = card_dma::fills();
                let from = head.saturating_sub(FILL_RING - 1).max(*seen);
                // B5's histogram is its writes'.
                let writing = matches!(side, Side::Stall { .. });
                let mut timed = 0;
                for us in (from..head).filter_map(card_dma::fill_time) {
                    timed += 1;
                    if !writing {
                        self.hist.record(us);
                    }
                }
                *lost += head.wrapping_sub(*seen) - timed;
                *seen = head;
                let due = |done: u32, every: u32| since >= (done + 1).saturating_mul(every);
                match side {
                    Side::Plain => {}
                    Side::Stall { writes, errors } => {
                        if let Some(ext) = stall
                            && due(*writes, WRITE_EVERY_MS)
                        {
                            let slot = *writes % (FILE_BLOCKS / FOUR_K.get());
                            let t0 = DWT::cycle_count();
                            match write_4k(store, ext, slot, &self.bufs.write) {
                                Ok(()) => {
                                    let took = DWT::cycle_count().wrapping_sub(t0);
                                    self.hist.record(cycles_us(self.cpu_hz, took));
                                }
                                Err(_) => *errors += 1,
                            }
                            *writes += 1;
                        }
                    }
                    Side::Ui {
                        last,
                        window,
                        passes,
                        fps_min,
                        frozen_ms,
                        saves,
                        ..
                    } => {
                        *frozen_ms = (*frozen_ms).max(now.since(*last));
                        *last = now;
                        *passes += 1;
                        if now.since(*window) >= 1_000 {
                            *fps_min = (*fps_min).min(*passes);
                            (*window, *passes) = (now, 0);
                        }
                        ask.redraw = true;
                        if due(*saves, SAVE_EVERY_MS) {
                            ask.save = true;
                            *saves += 1;
                        }
                    }
                    Side::Reads {
                        fault,
                        done,
                        read_at,
                        reads,
                        ok,
                    } => {
                        let read = match fault {
                            None if due(*done, WAKE_EVERY_MS) => {
                                *done += 1;
                                true
                            }
                            None => false,
                            Some(k) => {
                                if *done < INJECTIONS && due(*done, INJECT_EVERY_MS) {
                                    card_dma::inject(*k);
                                    *done += 1;
                                    *read_at = Some(now);
                                }
                                let late = read_at.is_some_and(|t| now.since(t) >= INJECT_READ_MS);
                                if late {
                                    *read_at = None;
                                }
                                late
                            }
                        };
                        if read {
                            *reads += 1;
                            if let (Some(v), Some(f)) = (*vol, *bed0) {
                                *ok += u32::from(ui_read(store, v, f));
                            }
                        }
                    }
                }
                since >= *ms
            }
        };
        if done {
            self.finish(stats);
        }
        ask
    }

    /// The run ended: its report kept, the streams and the chord off.
    fn finish(&mut self, stats: Option<&mut Reader<AudioStats>>) {
        let Some(run) = self.dma.run.take() else {
            return;
        };
        BENCH.disarm();
        // An injection no burst took would hit the next run's first.
        card_dma::clear_fault();
        self.chord(NoteKind::Off);
        let c = Counts::now().since(run.counts);
        let runs = self.dma.runs;
        let lines = |n, fills_lost| StreamLines {
            n_streams: n,
            report: BENCH.report(),
            fills_lost,
            runs,
        };
        let own = match run.work {
            Work::Chunk { points, .. } => {
                let fit = |pts: &[Option<(Bytes, Micros)>]| {
                    let mut got = [(Bytes::new(0), Micros::new(0)); B2_BLOCKS.len()];
                    let mut k = 0;
                    for (slot, p) in got.iter_mut().zip(pts.iter().flatten()) {
                        (*slot, k) = (*p, k + 1);
                    }
                    fit_line(&got[..k])
                };
                let (idle, loaded) = points.split_at(B2_BLOCKS.len());
                let t0 = DWT::cycle_count();
                black_box(crc16(black_box(&self.bufs.table[..Block::LEN])));
                DmaOwn::Chunk {
                    idle: fit(idle),
                    fit: fit(loaded),
                    crc16: cycles_us(self.cpu_hz, DWT::cycle_count().wrapping_sub(t0)),
                }
            }
            Work::Burst { .. } => DmaOwn::Burst,
            Work::Back { done, sums, .. } => {
                let mean = |sum: u64, n: u32| {
                    Micros::new(u32::try_from(sum / u64::from(n.max(1))).unwrap_or(u32::MAX))
                };
                DmaOwn::Back {
                    back_mean: mean(sums[0], done.min(BACK_READS)),
                    forward_mean: mean(sums[1], done.saturating_sub(BACK_READS)),
                }
            }
            Work::Streams { n, lost, side, .. } => match side {
                Side::Plain => DmaOwn::Streams(lines(n, lost)),
                Side::Stall { errors, .. } => DmaOwn::Stall {
                    streams: lines(n, lost),
                    write_busy_errors: errors,
                },
                Side::Ui {
                    fps_min,
                    frozen_ms,
                    save_ok,
                    ..
                } => DmaOwn::Ui {
                    streams: lines(n, lost),
                    // No window closed: a run stopped inside its first second.
                    ui_fps_min: if fps_min == u32::MAX { 0 } else { fps_min },
                    ui_frozen_max: Millis::new(frozen_ms),
                    save_ok,
                },
                Side::Reads { reads, ok, .. } => DmaOwn::Reads {
                    streams: lines(n, lost),
                    ui_reads: reads,
                    ui_reads_ok: ok,
                    faults: c.faults,
                },
            },
        };
        self.dma.last = Some(DmaReport {
            spread: self.hist.spread(),
            overruns: c.overruns,
            loaded_load_pct: load(stats).map_or(0, |l| l.load_pct),
            card: run.card.0,
            card_mb: run.card.1,
            aborts: c.aborts,
            stop_errors: c.stop_errors,
            card_irq_us_max: Micros::new(STATS.irq_us_max.load(Ordering::Relaxed)),
            card_irq_own_us_max: Micros::new(STATS.irq_own_us_max.load(Ordering::Relaxed)),
            irq: IrqCounts {
                entries: c.entries,
                reads: c.reads,
                bursts: c.bursts,
                bursts_max: STATS.bursts_max.load(Ordering::Relaxed),
            },
            own,
        });
    }
}

impl Run {
    fn pct(&self, now: Ms) -> u8 {
        let (done, of) = match &self.work {
            Work::Chunk { set, done, .. } => {
                (*set as u32 * B2_READS + done, B2_SETS as u32 * B2_READS)
            }
            Work::Burst { done } => (*done, BURSTS),
            Work::Back { done, .. } => (*done, 2 * BACK_READS),
            Work::Streams { ms, .. } => (now.since(self.began), *ms),
        };
        u8::try_from(u64::from(done) * 100 / u64::from(of.max(1))).unwrap_or(100)
    }
}

fn reads(fault: Option<Inject>) -> Side {
    Side::Reads {
        fault,
        done: 0,
        read_at: None,
        reads: 0,
        ok: 0,
    }
}

/// Bed `k` of `table`, from its source's runs and head: its name, rate
/// and run count, or `None` if it is missing, fragmented or not a `Plain`
/// source.
fn place(
    store: &mut SdStore,
    vol: VolumeId,
    table: &BedTable<'_>,
    k: usize,
    reg: &StreamRegistry,
    slot: &mut Option<BedCheck>,
) -> Option<(FileName, SourceRate, u32)> {
    let name = source_name(table.stem(k)?);
    let runs = Store::runs(store, vol, name).ok()?;
    let mut head = Head {
        buf: [0; HEAD_END],
        n: 0,
    };
    store.read(vol, name, &mut head).ok()?;
    let rec = head.buf.get(HEADER_LEN..HEAD_AT)?;
    let tag = u16::from_le_bytes([rec[0], rec[1]]);
    let len = usize::from(u16::from_le_bytes([rec[2], rec[3]]));
    if head.n < HEAD_END || tag != RecordTag::SourceHead.code() || len != SOURCE_HEAD_LEN {
        return None;
    }
    let sh = SourceHead::from_bytes(head.buf.get(HEAD_AT..)?)?;
    if sh.shape != Shape::Plain {
        return None;
    }
    let rate = SourceRate::new(sh.sref.rate)?;
    let file = StreamFile::from_layout(&SourceLayout::whole(sh)?, runs)?;
    let n = runs.ext.runs();
    table.place(k, file, reg, slot).then_some((name, rate, n))
}

/// A whole small file into `buf`; `over` if it doesn't fit.
struct Fill<'a> {
    buf: &'a mut [u8],
    n: usize,
    over: bool,
}

impl ReadSink for Fill<'_> {
    fn begin(&mut self, len: u32) -> ControlFlow<()> {
        self.over = usize::try_from(len).map_or(true, |l| l > self.buf.len());
        if self.over {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        let end = self.n + bytes.len();
        match self.buf.get_mut(self.n..end) {
            Some(out) => {
                out.copy_from_slice(bytes);
                self.n = end;
                ControlFlow::Continue(())
            }
            None => {
                self.over = true;
                ControlFlow::Break(())
            }
        }
    }
}

/// A source's first `HEAD_END` bytes.
struct Head {
    buf: [u8; HEAD_END],
    n: usize,
}

impl ReadSink for Head {
    fn begin(&mut self, _: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, bytes: &[u8]) -> ControlFlow<()> {
        let take = bytes.len().min(HEAD_END - self.n);
        self.buf[self.n..self.n + take].copy_from_slice(&bytes[..take]);
        self.n += take;
        if self.n == HEAD_END {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

/// The first chunk of a read, then a break.
struct First;

impl ReadSink for First {
    fn begin(&mut self, _: u32) -> ControlFlow<()> {
        ControlFlow::Continue(())
    }

    fn chunk(&mut self, _: &[u8]) -> ControlFlow<()> {
        ControlFlow::Break(())
    }
}

/// B9's and B10's UI read: bed 0's first block, through the store.
fn ui_read(store: &mut SdStore, vol: VolumeId, bed0: FileName) -> bool {
    store.read(vol, bed0, &mut First).is_ok()
}

/// B5's 4 KB at `slot` of the test file, split where a run ends.
fn write_4k(
    store: &mut SdStore,
    ext: &Extents,
    slot: u32,
    blocks: &[Block; WRITE_BLOCKS],
) -> Result<(), StoreError> {
    let from = FileBlock::new(slot * FOUR_K.get());
    for (lba, n) in ext.transfers(from, FOUR_K.get(), FOUR_K) {
        let k = usize::try_from(n.get()).unwrap_or(WRITE_BLOCKS);
        let part = blocks.get(..k).ok_or(StoreError::Io)?;
        store.device().with_device(|dev| {
            dev.start_op();
            dev.write(part, BlockIdx(lba.get()))
                .map_err(|e| dev.classify(&e))
        })??;
    }
    Ok(())
}
