//! `card` (bench builds): how fast the SD card reads while audio runs, for
//! sizing TAPE's and TRON's streams. The shell times raw block reads over
//! two test files' runs; this holds the plan, the sums and the text. KB is
//! 1000 B throughout.

use core::fmt::{self, Write};

use chimera_hal::store::StoreError;

use super::{Arg, Words};
use crate::dsp::xorshift32;

/// One 24 kHz 12-bit stream, in bytes a second.
pub const STREAM_BYTES_PER_S: u32 = 24_000 * 3 / 2;
/// Each test file, in 512 B blocks: 4 MiB.
pub const FILE_BLOCKS: u32 = 8_192;
pub const FILE_BYTES: u32 = FILE_BLOCKS * 512;
/// The test files, aimed this far (per mille) into the card's clusters so
/// a read that switches between them crosses half the card.
pub const FILE_AIMS: [u16; 2] = [250, 750];
pub const FILES: usize = FILE_AIMS.len();
/// Free space a test file must leave behind: the card never fills.
pub const ROOM_MARGIN_BYTES: u32 = 1 << 20;
/// The sequential transfers, in blocks: 512 B, 4 KB and 16 KB.
pub const SEQ_BLOCKS: [u32; 3] = [1, 8, 32];
/// Each sequential run's share of the first file, in blocks (768 KiB), so
/// no run reads what the card may still hold from the last.
pub const SPAN_BLOCKS: u32 = 1_536;
/// A random read and a stream's refill, in blocks: 4 KB, 4 KB aligned.
pub const READ_BLOCKS: u32 = 8;
pub const RANDOM_READS: usize = 1_024;
/// TRON's pattern: this many streams, each refilled 4 KB in turn, each
/// from its own place, half in each file.
pub const STREAMS: u32 = 8;
pub const STREAM_ROUNDS: u32 = 24;
/// Runs `Extents` keeps per file; a file in more is `CardFail::Fragmented`.
pub const MAX_RUNS: usize = 32;
/// The whole `card`, files written included: well inside the host's 60 s.
pub const DEADLINE_MS: u32 = 45_000;
/// A span timed by the cycle counter must end before it wraps (8.9 s at
/// 480 MHz); one this long in ms is refused, not misread.
pub const CYCLE_SPAN_MS: u32 = 8_000;

const _: () = assert!(SEQ_BLOCKS.len() as u32 * SPAN_BLOCKS <= FILE_BLOCKS);
const _: () = assert!(FILE_BLOCKS.is_multiple_of(READ_BLOCKS));
const _: () = assert!(STREAM_ROUNDS * READ_BLOCKS <= FILE_BLOCKS / (STREAMS / FILES as u32));

/// The file block where sequential run `i` starts.
pub const fn span_start(i: u32) -> u32 {
    i * SPAN_BLOCKS
}

/// `card`'s argument: measure (with a seed for the random reads), or
/// remove `/CHIMERA/BENCH`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardArg {
    Run(Option<u32>),
    Clean,
}

impl Arg for CardArg {
    const USAGE: &'static str = "clean, seed=N or nothing";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let got = match words.next() {
            None => CardArg::Run(None),
            Some(w) if w.eq_ignore_ascii_case(b"clean") => CardArg::Clean,
            Some(w) => {
                let n = w.strip_prefix(b"seed=")?;
                let n = core::str::from_utf8(n).ok()?.parse().ok()?;
                CardArg::Run(Some(n))
            }
        };
        words.next().is_none().then_some(got)
    }
}

/// Bytes moved in a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Throughput {
    pub bytes: u32,
    pub us: u32,
}

impl Throughput {
    pub fn bytes_per_s(self) -> u32 {
        let r = u64::from(self.bytes) * 1_000_000 / u64::from(self.us.max(1));
        u32::try_from(r).unwrap_or(u32::MAX)
    }

    pub fn kb_per_s(self) -> u32 {
        self.bytes_per_s() / 1_000
    }
}

/// A read's latency spread, in µs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Latency {
    pub min_us: u32,
    pub median_us: u32,
    pub p95_us: u32,
    pub p99_us: u32,
    pub max_us: u32,
}

impl Latency {
    /// Of `us`, which it sorts; `None` if empty. The median is the upper
    /// one of an even count, the percentiles nearest rank.
    pub fn of(us: &mut [u32]) -> Option<Latency> {
        us.sort_unstable();
        let n = us.len();
        let rank = |pct: usize| (n * pct).div_ceil(100).max(1) - 1;
        Some(Latency {
            min_us: *us.first()?,
            median_us: *us.get(n / 2)?,
            p95_us: *us.get(rank(95))?,
            p99_us: *us.get(rank(99))?,
            max_us: *us.last()?,
        })
    }
}

/// A span timed by the cycle counter, in µs: `None` once it ran
/// `CYCLE_SPAN_MS` by the ms clock, when the counter may have wrapped.
pub fn span_us(cycles: u32, cpu_hz: u32, ms: u32) -> Option<u32> {
    (ms < CYCLE_SPAN_MS).then(|| cycles / (cpu_hz / 1_000_000).max(1))
}

/// The card's class, from its addressing and size (SD spec: SDHC is block
/// addressed up to 32 GB, SDXC past it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardKind {
    Sdsc,
    Sdhc,
    Sdxc,
}

impl CardKind {
    pub fn of(block_addressed: bool, bytes: u64) -> CardKind {
        match (block_addressed, bytes) {
            (false, _) => CardKind::Sdsc,
            (true, b) if b <= 32_000_000_000 => CardKind::Sdhc,
            (true, _) => CardKind::Sdxc,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CardKind::Sdsc => "SDSC",
            CardKind::Sdhc => "SDHC",
            CardKind::Sdxc => "SDXC",
        }
    }
}

/// The card itself. The driver doesn't read the CID, so no maker or
/// product name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CardInfo {
    pub kind: CardKind,
    pub bytes: u64,
    /// Free once the test files are on it.
    pub free_bytes: u64,
}

/// The test files as found or written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Files {
    /// Runs across both files.
    pub runs: u32,
    /// Files this `card` wrote.
    pub created: u8,
    /// How far apart they start on the card, in MiB.
    pub apart_mib: u32,
}

/// Audio while the chord played.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Load {
    pub voices: u8,
    pub load_pct: u16,
}

/// What `card` measured: each phase `None` until it completes, so a
/// timeout still reports what came before.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CardSpeed {
    pub seed: u32,
    pub card: Option<CardInfo>,
    pub files: Option<Files>,
    /// Per `SEQ_BLOCKS`, no chord.
    pub seq: [Option<Throughput>; SEQ_BLOCKS.len()],
    /// `RANDOM_READS` 4 KB reads across both files, the chord playing.
    pub random: Option<Latency>,
    /// TRON's pattern (`streams`), the chord playing.
    pub interleaved: Option<Throughput>,
    pub load: Option<Load>,
    /// Audio blocks missed while `card` ran.
    pub overruns: u32,
}

impl CardSpeed {
    /// 24 kHz 12-bit streams at half the interleaved rate: TRON's pattern
    /// under load, with a 50 % margin.
    pub fn streams(&self) -> Option<u32> {
        self.interleaved
            .map(|t| t.bytes_per_s() / 2 / STREAM_BYTES_PER_S)
    }
}

/// Why `card` stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardFail {
    /// No audio block rendered: `card` runs only beside audio.
    NoAudio,
    /// A test file lies in more than `MAX_RUNS` runs.
    Fragmented,
    /// Less free than a test file and `ROOM_MARGIN_BYTES`: nothing written.
    NoRoom,
    /// A test file's write ran out of time; it was deleted.
    SlowWrite,
    /// `DEADLINE_MS` passed.
    Timeout,
    /// A timed span ran past the cycle counter's wrap.
    ClockWrap,
    /// `card clean` left `/CHIMERA/BENCH` holding files it can't name.
    NotEmpty,
    Store(StoreError),
}

impl CardFail {
    /// `ERR card: <this>`.
    pub fn message(self) -> &'static str {
        match self {
            CardFail::NoAudio => "audio is not running",
            CardFail::Fragmented => "test file too fragmented: run card clean",
            CardFail::NoRoom => "under 5 MiB free: no test file written",
            CardFail::SlowWrite => "card too slow to write a test file: deleted",
            CardFail::Timeout => "timeout",
            CardFail::ClockWrap => "a timed span outran the cycle counter",
            CardFail::NotEmpty => "BENCH holds other files: delete them on a computer",
            CardFail::Store(e) => e.message(),
        }
    }
}

impl From<StoreError> for CardFail {
    fn from(e: StoreError) -> Self {
        CardFail::Store(e)
    }
}

/// What a `card` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardAnswer {
    /// What was measured, and why it stopped short if it did.
    Measured(CardSpeed, Option<CardFail>),
    Cleaned(Result<(), CardFail>),
}

/// `card`'s body: `key value` lines, for the phases that completed.
pub fn write_card(s: &CardSpeed, t: &mut impl Write) -> fmt::Result {
    writeln!(t, "seed {}", s.seed)?;
    if let Some(c) = &s.card {
        writeln!(t, "card_type {}", c.kind.name())?;
        writeln!(t, "card_mb {}", c.bytes / 1_000_000)?;
        writeln!(t, "free_mb {}", c.free_bytes / 1_000_000)?;
    }
    if let Some(f) = &s.files {
        writeln!(t, "file_kb {}", FILES as u32 * FILE_BYTES / 1_000)?;
        writeln!(t, "file_runs {}", f.runs)?;
        writeln!(t, "file_created {}", f.created)?;
        writeln!(t, "file_apart_mib {}", f.apart_mib)?;
    }
    for (blocks, tp) in SEQ_BLOCKS.iter().zip(&s.seq) {
        if let Some(tp) = tp {
            writeln!(t, "seq_{}_kb_s {}", Size(*blocks * 512), tp.kb_per_s())?;
        }
    }
    if let Some(r) = &s.random {
        writeln!(t, "rand_4k_min_us {}", r.min_us)?;
        writeln!(t, "rand_4k_median_us {}", r.median_us)?;
        writeln!(t, "rand_4k_p95_us {}", r.p95_us)?;
        writeln!(t, "rand_4k_p99_us {}", r.p99_us)?;
        writeln!(t, "rand_4k_max_us {}", r.max_us)?;
    }
    if let Some(i) = s.interleaved {
        writeln!(t, "streams_kb_s {}", i.kb_per_s())?;
    }
    if let Some(l) = &s.load {
        writeln!(t, "loaded_voices {}", l.voices)?;
        writeln!(t, "loaded_load_pct {}", l.load_pct)?;
    }
    writeln!(t, "overruns {}", s.overruns)?;
    match s.streams() {
        Some(n) => writeln!(t, "streams {n}"),
        None => Ok(()),
    }
}

/// A transfer's name: `512`, `4k`, `16k`.
struct Size(u32);

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            b if b >= 1_024 && b.is_multiple_of(1_024) => write!(f, "{}k", b / 1_024),
            b => write!(f, "{b}"),
        }
    }
}

/// A file's runs on the device, as `FatStore::runs` gives them.
#[derive(Clone, Copy, Debug)]
pub struct Extents {
    runs: [(u32, u32); MAX_RUNS],
    kept: usize,
    seen: u32,
}

impl Default for Extents {
    fn default() -> Self {
        Self::new()
    }
}

impl Extents {
    pub const fn new() -> Self {
        Extents {
            runs: [(0, 0); MAX_RUNS],
            kept: 0,
            seen: 0,
        }
    }

    /// The next run: its first device block and its length in blocks.
    pub fn push(&mut self, first: u32, blocks: u32) {
        if let Some(slot) = self.runs.get_mut(self.kept) {
            *slot = (first, blocks);
            self.kept += 1;
        }
        self.seen = self.seen.saturating_add(1);
    }

    /// Every run pushed, kept or not.
    pub fn seen(&self) -> u32 {
        self.seen
    }

    /// Whether every run pushed was kept.
    pub fn whole(&self) -> bool {
        self.seen as usize == self.kept
    }

    /// The first run's first device block.
    pub fn first(&self) -> Option<u32> {
        self.runs.get(..self.kept)?.first().map(|r| r.0)
    }

    /// File block `at` on the device, and the blocks left in its run.
    pub fn locate(&self, mut at: u32) -> Option<(u32, u32)> {
        for &(first, n) in self.runs.get(..self.kept)? {
            if at < n {
                return Some((first + at, n - at));
            }
            at -= n;
        }
        None
    }

    /// The reads of file blocks `from..from + len`, each at most `max`
    /// blocks and none across a run's end: (device block, blocks). Stops
    /// short past the kept runs.
    pub fn transfers(&self, from: u32, len: u32, max: u32) -> Transfers<'_> {
        Transfers {
            ext: self,
            at: from,
            end: from.saturating_add(len),
            max: max.max(1),
        }
    }
}

pub struct Transfers<'a> {
    ext: &'a Extents,
    at: u32,
    end: u32,
    max: u32,
}

impl Iterator for Transfers<'_> {
    type Item = (u32, u32);

    fn next(&mut self) -> Option<(u32, u32)> {
        if self.at >= self.end {
            return None;
        }
        let (lba, left) = self.ext.locate(self.at)?;
        let n = left.min(self.max).min(self.end - self.at);
        self.at += n;
        Some((lba, n))
    }
}

/// `RANDOM_READS` 4 KB reads from `seed`: (file, its 4 KB-aligned first
/// block), spread over both files.
pub fn random_reads(seed: u32) -> impl Iterator<Item = (usize, u32)> {
    // Spread by an odd constant first: no two seeds share a state.
    let mut state = seed.wrapping_mul(0x9E37_79B9) | 1;
    let slots = FILE_BLOCKS / READ_BLOCKS;
    (0..RANDOM_READS).map(move |_| {
        let r = xorshift32(&mut state);
        ((r >> 31) as usize, (r % slots) * READ_BLOCKS)
    })
}

/// TRON's pattern: `STREAM_ROUNDS` rounds of each of `STREAMS` streams
/// reading its next 4 KB, (file, first block). Stream `i` plays from file
/// `i % 2`, its own quarter.
pub fn interleaved() -> impl Iterator<Item = (usize, u32)> {
    let per_file = STREAMS / FILES as u32;
    let stride = FILE_BLOCKS / per_file;
    (0..STREAM_ROUNDS).flat_map(move |round| {
        (0..STREAMS).map(move |i| {
            let file = (i as usize) % FILES;
            (file, (i / FILES as u32) * stride + round * READ_BLOCKS)
        })
    })
}
