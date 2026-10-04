//! The console's `card` on the unit (bench builds): the SD driver's own
//! multi-block reads over the test file's runs, timed by the cycle counter,
//! from the UI loop while audio runs. Every read keeps `SD_IDLE_MS`; audio,
//! MIDI and the watchdog's kicks run above it in interrupts throughout.

use core::ptr::addr_of_mut;
use core::sync::atomic::{AtomicBool, Ordering};

use chimera_core::boot::wait_until;
use chimera_core::console::{
    CardFail, CardSpeed, Extents, FILE_BLOCKS, LOADED_BLOCKS, Latency, RANDOM_BLOCKS, RANDOM_READS,
    SEQ_BLOCKS, SPAN_BLOCKS, Throughput, random_starts, span_start,
};
use chimera_core::note_queue::{NoteEvent, NoteKind, NoteProducer};
use chimera_core::perf::load::AudioStats;
use chimera_core::triple::Reader;
use chimera_core::{MidiChannel, MidiNote, Velocity};
use chimera_fat::Medium;
use chimera_hal::store::{ByteSink, CHUNK, Dir, FileName, Store, StoreError, VolumeId};
use cortex_m::peripheral::DWT;
use embedded_sdmmc::{Block, BlockDevice, BlockIdx};

use crate::audio::dma::{BLOCKS, OVERRUNS};
use crate::sd::{SD_OP_CAP_MS, SdStore};

/// The largest transfer, 16 KB.
const BUF_BLOCKS: usize = 32;
const _: () = assert!(LOADED_BLOCKS as usize <= BUF_BLOCKS);
const _: () = assert!(RANDOM_BLOCKS as usize <= BUF_BLOCKS);
/// Audio counts as running if it renders a block within this.
const LIVE_MS: u32 = 10;
/// The test file's write in all: 4 MiB at the 363 KB/s ADR 0045 measured
/// is about 12 s.
const CREATE_CAP_MS: u32 = 40_000;
/// The chord's voices start before the loaded run is timed.
const SETTLE_US: u32 = 100_000;
/// Eight voices on channel 1, an octave and a half of C major sevenths.
const CHORD: [u8; 8] = [48, 52, 55, 59, 60, 64, 67, 71];

// A static, not the stack: 16 KB would trip the stack check.
static mut BUF: [Block; BUF_BLOCKS] = [const {
    Block {
        contents: [0; Block::LEN],
    }
}; BUF_BLOCKS];
static BUF_TAKEN: AtomicBool = AtomicBool::new(false);

pub struct CardBench {
    buf: &'static mut [Block; BUF_BLOCKS],
    notes: NoteProducer<'static>,
    cpu_hz: u32,
}

/// The one `CardBench`, over the `CARD` note source.
pub fn take(notes: NoteProducer<'static>, cpu_hz: u32) -> Option<CardBench> {
    if BUF_TAKEN.swap(true, Ordering::AcqRel) {
        return None;
    }
    // SAFETY: the flag lets exactly one caller past, so this is the only
    // reference to `BUF` ever made.
    let buf = unsafe { &mut *addr_of_mut!(BUF) };
    Some(CardBench { buf, notes, cpu_hz })
}

fn test_file() -> FileName {
    FileName::new(Dir::Bench, b"CARDTEST", b"BIN").expect("an 8.3 name")
}

impl CardBench {
    /// The whole measurement; `stats` gives the loaded run's audio load.
    pub fn measure(
        &mut self,
        store: &mut SdStore,
        stats: Option<&mut Reader<AudioStats>>,
    ) -> Result<CardSpeed, CardFail> {
        let seen = BLOCKS.load(Ordering::Relaxed);
        let live = wait_until(LIVE_MS * (self.cpu_hz / 1_000), DWT::cycle_count, || {
            BLOCKS.load(Ordering::Relaxed) != seen
        });
        if !live {
            return Err(CardFail::NoAudio);
        }
        let overruns = OVERRUNS.load(Ordering::Relaxed);
        let vol = store.mount()?;
        store.make_dir(vol, Dir::Chimera)?;
        store.make_dir(vol, Dir::Bench)?;
        let (ext, file_bytes, created) = test_runs(store, vol)?;
        let mut s = CardSpeed {
            file_bytes,
            runs: ext.seen(),
            created,
            ..CardSpeed::default()
        };
        for ((i, &n), out) in (0..).zip(&SEQ_BLOCKS).zip(&mut s.seq) {
            *out = self.sequential(store, &ext, span_start(i), n)?;
        }
        s.random = self.random(store, &ext)?;
        self.chord(NoteKind::On(Velocity::DEFAULT));
        crate::clocks::delay_us(self.cpu_hz, SETTLE_US);
        let loaded = self.sequential(
            store,
            &ext,
            span_start(SEQ_BLOCKS.len() as u32),
            LOADED_BLOCKS,
        );
        if let Some(a) = crate::audio_stats(stats) {
            (s.voices, s.load_pct) = (a.voices, a.load_avg);
        }
        self.chord(NoteKind::Off);
        s.loaded = loaded?;
        s.overruns = OVERRUNS.load(Ordering::Relaxed).wrapping_sub(overruns);
        Ok(s)
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
    ) -> Result<Throughput, CardFail> {
        let t = DWT::cycle_count();
        for (lba, n) in ext.transfers(from, SPAN_BLOCKS, max) {
            self.read(store, lba, n)?;
        }
        Ok(Throughput {
            bytes: SPAN_BLOCKS * Block::LEN_U32,
            us: self.us_since(t),
        })
    }

    /// `RANDOM_READS` reads of 4 KB, each timed.
    fn random(&mut self, store: &SdStore, ext: &Extents) -> Result<Latency, CardFail> {
        let mut us = [0u32; RANDOM_READS];
        for (slot, at) in us.iter_mut().zip(random_starts(DWT::cycle_count())) {
            let t = DWT::cycle_count();
            for (lba, n) in ext.transfers(at, RANDOM_BLOCKS, RANDOM_BLOCKS) {
                self.read(store, lba, n)?;
            }
            *slot = self.us_since(t);
        }
        Ok(Latency::of(&mut us).unwrap_or_default())
    }

    /// One multi-block read, under its own `SD_IDLE_MS`. A failure
    /// re-inits the card, as `FatStore` does.
    fn read(&mut self, store: &SdStore, lba: u32, n: u32) -> Result<(), CardFail> {
        let dev = store.device();
        let blocks = self.buf.get_mut(..n as usize).ok_or(StoreError::Io)?;
        dev.start_op();
        dev.read(blocks, BlockIdx(lba)).map_err(|e| {
            let why = dev.classify(&e);
            dev.reinit();
            CardFail::Store(why)
        })
    }

    fn us_since(&self, t: u32) -> u32 {
        DWT::cycle_count().wrapping_sub(t) / (self.cpu_hz / 1_000_000).max(1)
    }
}

/// The test file's runs, its length, and whether this call wrote it: an
/// existing file of `FILE_BLOCKS` is used as it is.
fn test_runs(store: &mut SdStore, vol: VolumeId) -> Result<(Extents, u32, bool), CardFail> {
    let want = FILE_BLOCKS * Block::LEN_U32;
    let mut created = false;
    loop {
        let mut ext = Extents::new();
        match store.runs(vol, test_file(), &mut |at, n| ext.push(at, n)) {
            Ok(len) if len >= want => {
                if !ext.whole() {
                    return Err(CardFail::Fragmented);
                }
                return Ok((ext, len, created));
            }
            Ok(_) | Err(StoreError::NotFound) if !created => {}
            Ok(_) => return Err(StoreError::Corrupt.into()),
            Err(e) => return Err(e.into()),
        }
        write_test_file(store, vol, want)?;
        created = true;
    }
}

/// `bytes` of a counting pattern, under `CREATE_CAP_MS`.
fn write_test_file(store: &mut SdStore, vol: VolumeId, bytes: u32) -> Result<(), StoreError> {
    store.device().spi(|s| s.set_cap_ms(CREATE_CAP_MS));
    let wrote = store.write(vol, test_file(), &mut |sink: &mut dyn ByteSink| {
        let mut chunk = [0u8; CHUNK];
        for i in 0..bytes / CHUNK as u32 {
            chunk.fill(i as u8);
            sink.put(&chunk)?;
        }
        Ok(())
    });
    store.device().spi(|s| s.set_cap_ms(SD_OP_CAP_MS));
    wrote.map(|_| ())
}
