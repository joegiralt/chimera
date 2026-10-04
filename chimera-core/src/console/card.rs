//! `card` (bench builds): how fast the SD card reads while audio runs, for
//! sizing the sound pool's streams. The shell times raw block reads over a
//! test file's runs; this holds the plan, the sums and the text. KB is
//! 1000 B throughout.

use core::fmt::{self, Write};

use chimera_hal::store::StoreError;

use crate::dsp::xorshift32;

/// One 24 kHz 12-bit stream, in bytes a second.
pub const STREAM_BYTES_PER_S: u32 = 24_000 * 3 / 2;
/// The test file, in 512 B blocks: 4 MiB.
pub const FILE_BLOCKS: u32 = 8_192;
/// The sequential transfers, in blocks: 512 B, 4 KB and 16 KB.
pub const SEQ_BLOCKS: [u32; 3] = [1, 8, 32];
/// The loaded run's transfer: 16 KB.
pub const LOADED_BLOCKS: u32 = 32;
/// Each timed run's share of the file, in blocks (768 KiB). Each run reads
/// its own, so no run reads what the card may still hold from the last.
pub const SPAN_BLOCKS: u32 = 1_536;
/// The random read, in blocks: 4 KB, 4 KB aligned.
pub const RANDOM_BLOCKS: u32 = 8;
pub const RANDOM_READS: usize = 256;
/// Runs `Extents` keeps; a file in more is `CardFail::Fragmented`.
pub const MAX_RUNS: usize = 64;

// The three sequential runs and the loaded one fit the file.
const _: () = assert!((SEQ_BLOCKS.len() as u32 + 1) * SPAN_BLOCKS <= FILE_BLOCKS);
const _: () = assert!(FILE_BLOCKS.is_multiple_of(RANDOM_BLOCKS));

/// The file block where sequential run `i` (or the loaded run, `i` = 3)
/// starts.
pub const fn span_start(i: u32) -> u32 {
    i * SPAN_BLOCKS
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
    pub p99_us: u32,
    pub max_us: u32,
}

impl Latency {
    /// Of `us`, which it sorts; `None` if empty. The median is the upper
    /// one of an even count, p99 the nearest rank.
    pub fn of(us: &mut [u32]) -> Option<Latency> {
        us.sort_unstable();
        let n = us.len();
        let p99 = (n * 99).div_ceil(100).max(1) - 1;
        Some(Latency {
            min_us: *us.first()?,
            median_us: *us.get(n / 2)?,
            p99_us: *us.get(p99)?,
            max_us: *us.last()?,
        })
    }
}

/// What `card` measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CardSpeed {
    pub file_bytes: u32,
    /// Contiguous runs the test file lies in.
    pub runs: u32,
    /// This `card` wrote the test file.
    pub created: bool,
    /// Per `SEQ_BLOCKS`.
    pub seq: [Throughput; SEQ_BLOCKS.len()],
    pub random: Latency,
    /// 16 KB transfers with the chord playing.
    pub loaded: Throughput,
    /// Voices sounding and the audio load (the AUDIO LOAD page's average)
    /// at the loaded run's end.
    pub voices: u8,
    pub load_pct: u16,
    /// Audio blocks missed while `card` ran.
    pub overruns: u32,
}

impl CardSpeed {
    /// Streams at half the loaded rate.
    pub fn streams_seq(&self) -> u32 {
        self.loaded.bytes_per_s() / 2 / STREAM_BYTES_PER_S
    }

    /// Streams at half the rate of 4 KB refills that each wait the p99
    /// latency.
    pub fn streams_random(&self) -> u32 {
        Throughput {
            bytes: RANDOM_BLOCKS * 512,
            us: self.random.p99_us,
        }
        .bytes_per_s()
            / 2
            / STREAM_BYTES_PER_S
    }

    /// The lower of the two: streams the card holds with a 50% margin.
    pub fn streams(&self) -> u32 {
        self.streams_seq().min(self.streams_random())
    }
}

/// Why `card` measured nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardFail {
    /// No audio block rendered: `card` runs only beside audio.
    NoAudio,
    /// The test file lies in more than `MAX_RUNS` runs.
    Fragmented,
    Store(StoreError),
}

impl CardFail {
    /// `ERR card: <this>`.
    pub fn message(self) -> &'static str {
        match self {
            CardFail::NoAudio => "audio is not running",
            CardFail::Fragmented => "test file too fragmented: delete it or format the card",
            CardFail::Store(e) => e.message(),
        }
    }
}

impl From<StoreError> for CardFail {
    fn from(e: StoreError) -> Self {
        CardFail::Store(e)
    }
}

/// `card`'s body: `key value` lines.
pub fn write_card(s: &CardSpeed, t: &mut impl Write) -> fmt::Result {
    writeln!(t, "file_kb {}", s.file_bytes / 1_000)?;
    writeln!(t, "file_runs {}", s.runs)?;
    writeln!(t, "file_created {}", u8::from(s.created))?;
    for (blocks, tp) in SEQ_BLOCKS.iter().zip(&s.seq) {
        writeln!(t, "seq_{}_kb_s {}", size(*blocks), tp.kb_per_s())?;
    }
    let r = &s.random;
    writeln!(t, "rand_4k_min_us {}", r.min_us)?;
    writeln!(t, "rand_4k_median_us {}", r.median_us)?;
    writeln!(t, "rand_4k_p99_us {}", r.p99_us)?;
    writeln!(t, "rand_4k_max_us {}", r.max_us)?;
    writeln!(t, "loaded_16k_kb_s {}", s.loaded.kb_per_s())?;
    writeln!(t, "loaded_voices {}", s.voices)?;
    writeln!(t, "loaded_load_pct {}", s.load_pct)?;
    writeln!(t, "overruns {}", s.overruns)?;
    writeln!(t, "streams_seq {}", s.streams_seq())?;
    writeln!(t, "streams_rand {}", s.streams_random())?;
    writeln!(t, "streams {}", s.streams())
}

/// A transfer's name: `512`, `4k`, `16k`.
fn size(blocks: u32) -> Size {
    Size(blocks * 512)
}

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

/// `RANDOM_READS` 4 KB-aligned file blocks, spread over `FILE_BLOCKS`,
/// from `seed`.
pub fn random_starts(seed: u32) -> impl Iterator<Item = u32> {
    let mut state = seed | 1;
    let slots = FILE_BLOCKS / RANDOM_BLOCKS;
    (0..RANDOM_READS).map(move |_| xorshift32(&mut state) % slots * RANDOM_BLOCKS)
}
