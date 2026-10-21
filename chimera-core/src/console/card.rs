//! `card` (bench builds): how fast the SD card reads while audio runs, for
//! sizing TAPE's and TRON's streams. The shell times raw block reads over
//! two test files' runs; this holds the plan, the sums and the text. KB is
//! 1000 B throughout.

use core::fmt::{self, Write};

use chimera_hal::store::StoreError;
pub use chimera_hal::store::{Extents, MAX_RUNS, Runs};
use chimera_hal::units::{Bytes, BytesPerSec, Micros, Millis, StreamCount};

use crate::stream::bench::BenchReport;
use crate::ui::about_page::THIS;

use super::{Arg, Words};

/// One 24 kHz 12-bit stream, in bytes a second.
pub const STREAM_BYTES_PER_S: u32 = 24_000 * 3 / 2;
/// Each test file, in 512 B blocks: 4 MiB.
pub const FILE_BLOCKS: u32 = 8_192;
pub const FILE_BYTES: u32 = FILE_BLOCKS * 512;
// The sweep's cap counts on it (ADR 0094).
const _: () = assert!(FILE_BYTES <= chimera_hal::store::MAX_CHAIN_BYTES);
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
pub const STREAM_ROUNDS: u32 = 100;
/// The test files must start at least this far apart (per mille of the
/// card), or `card` fails with `TooClose`; it doesn't retry placement.
pub const MIN_APART_PER_MILLE: u64 = 250;
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

/// `card`'s argument: measure (with a seed for the random reads), remove
/// `/CHIMERA/BENCH`, or a bench on the DMA driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardArg {
    Run(Option<u32>),
    Clean,
    Dma(DmaVerb),
}

impl Arg for CardArg {
    const USAGE: &'static str = "clean, seed=N, dma <chunk|burst|sustain N|stall N|ui N|back|listen N|wake N|inject abort|te|modf|report|stop> or nothing";
    fn parse(mut words: Words<'_>) -> Option<Self> {
        let got = match words.next() {
            None => CardArg::Run(None),
            Some(w) if w.eq_ignore_ascii_case(b"clean") => CardArg::Clean,
            Some(w) if w.eq_ignore_ascii_case(b"dma") => CardArg::Dma(DmaVerb::parse(&mut words)?),
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

/// Ciura's gaps, then insertion: a few dozen bytes, where `sort_unstable`
/// links 3.6 KB of quicksort for 1 024 timings.
fn shell_sort(v: &mut [u32]) {
    for gap in [701, 301, 132, 57, 23, 10, 4, 1] {
        for i in gap..v.len() {
            let x = v[i];
            let mut j = i;
            while j >= gap && v[j - gap] > x {
                v[j] = v[j - gap];
                j -= gap;
            }
            v[j] = x;
        }
    }
}

impl Latency {
    /// Of `us`, which it sorts; `None` if empty. The median is the upper
    /// one of an even count, the percentiles nearest rank.
    pub fn of(us: &mut [u32]) -> Option<Latency> {
        shell_sort(us);
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

/// How far apart two device blocks are, per mille of a card of `bytes`.
pub fn apart_per_mille(a: u32, b: u32, bytes: u64) -> u32 {
    let blocks = (bytes / 512).max(1);
    (u64::from(a.abs_diff(b)) * 1_000 / blocks) as u32
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
    /// Free once the test files are on it; `None` when both were there
    /// already and the FAT wasn't scanned.
    pub free_bytes: Option<u64>,
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
    /// The same, per mille of the card.
    pub apart_per_mille: u32,
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
    /// TRON's pattern, the chord playing: the mean rate, and the spread of
    /// a round's time (every stream refilled once).
    pub interleaved: Option<Throughput>,
    pub rounds: Option<Latency>,
    pub load: Option<Load>,
    /// Audio blocks missed while `card` ran.
    pub overruns: u32,
}

impl CardSpeed {
    /// 24 kHz 12-bit streams at half the rate of a p95 round: TRON's
    /// pattern under load, its tail with a 50 % margin, not its mean.
    pub fn streams(&self) -> Option<u32> {
        let round = Throughput {
            bytes: STREAMS * READ_BLOCKS * 512,
            us: self.rounds?.p95_us,
        };
        Some(round.bytes_per_s() / 2 / STREAM_BYTES_PER_S)
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
    /// The test files start under `MIN_APART_PER_MILLE` of the card apart.
    TooClose,
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
            CardFail::TooClose => "test files under a quarter of the card apart: run card clean",
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
    Dma(DmaAnswer),
}

/// `card`'s body: `key value` lines, for the phases that completed.
pub fn write_card(s: &CardSpeed, t: &mut impl Write) -> fmt::Result {
    writeln!(t, "seed {}", s.seed)?;
    if let Some(c) = &s.card {
        writeln!(t, "card_type {}", c.kind.name())?;
        writeln!(t, "card_mb {}", c.bytes / 1_000_000)?;
        if let Some(free) = c.free_bytes {
            writeln!(t, "free_mb {}", free / 1_000_000)?;
        }
    }
    if let Some(f) = &s.files {
        writeln!(t, "file_kb {}", FILES as u32 * FILE_BYTES / 1_000)?;
        writeln!(t, "file_runs {}", f.runs)?;
        writeln!(t, "file_created {}", f.created)?;
        writeln!(t, "file_apart_mib {}", f.apart_mib)?;
        writeln!(t, "file_apart_per_mille {}", f.apart_per_mille)?;
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
    if let Some(r) = &s.rounds {
        writeln!(t, "round_median_us {}", r.median_us)?;
        writeln!(t, "round_p95_us {}", r.p95_us)?;
        writeln!(t, "round_max_us {}", r.max_us)?;
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

/// `RANDOM_READS` 4 KB reads from `seed`: (file, its 4 KB-aligned first
/// block), spread over both files.
pub fn random_reads(seed: u32) -> impl Iterator<Item = (usize, u32)> {
    // Xorshift64 (13, 7, 17) from the seed in the low word and 1 above
    // it: never zero, and no two seeds share a state.
    let mut state = 1 << 32 | u64::from(seed);
    let slots = FILE_BLOCKS / READ_BLOCKS;
    (0..RANDOM_READS).map(move |_| {
        let r = xorshift64(&mut state);
        ((r >> 31) as usize, (r % slots) * READ_BLOCKS)
    })
}

/// Xorshift64 (13, 7, 17): the next state, its high word drawn. A zero
/// state stays zero, so seed it.
#[inline(always)]
pub fn xorshift64(state: &mut u64) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 32) as u32
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

/// A bench on the DMA driver (slice 0's B2–B10). Stream k reads bed k,
/// the beds `/CHIMERA/BENCH/BEDS.CHK` lists: no verb names one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaVerb {
    /// B2: random reads of 1 to 16 KB, idle and loaded; the line fit.
    Chunk,
    /// B3: eight reads queued at once, the 8th's time.
    Burst,
    /// B4: N streams for 10 min.
    Sustain(StreamCount),
    /// B5: B4 with a 4 KB write a second.
    Stall(StreamCount),
    /// B6: B4 with the UI redrawn whole every pass and a save a minute.
    Ui(StreamCount),
    /// B7: a backward 4 KB walk, against forward random.
    Back,
    /// B8: B4 with stream 0 played aloud.
    Listen(StreamCount),
    /// B10: B4 with a UI read every 50 ms.
    Wake(StreamCount),
    /// B9: B4 at 2 with a fault a second.
    Inject(Inject),
    Report,
    Stop,
}

/// B9's fault, armed for one transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inject {
    /// An abort asked mid-CMD18.
    Abort,
    /// One RX burst aimed at DTCM, which DMA2 can't reach.
    Te,
    /// CR1 written once without SSI.
    Modf,
}

impl Inject {
    pub fn name(self) -> &'static str {
        match self {
            Inject::Abort => "abort",
            Inject::Te => "te",
            Inject::Modf => "modf",
        }
    }
}

impl DmaVerb {
    /// The words after `dma`, all of them.
    fn parse(words: &mut Words<'_>) -> Option<DmaVerb> {
        let w = words.next()?;
        let is = |name: &str| w.eq_ignore_ascii_case(name.as_bytes());
        let mut count = || {
            let n = core::str::from_utf8(words.next()?).ok()?.parse().ok()?;
            StreamCount::new(n)
        };
        let verb = if is("chunk") {
            DmaVerb::Chunk
        } else if is("burst") {
            DmaVerb::Burst
        } else if is("sustain") {
            DmaVerb::Sustain(count()?)
        } else if is("stall") {
            DmaVerb::Stall(count()?)
        } else if is("ui") {
            DmaVerb::Ui(count()?)
        } else if is("back") {
            DmaVerb::Back
        } else if is("listen") {
            DmaVerb::Listen(count()?)
        } else if is("wake") {
            DmaVerb::Wake(count()?)
        } else if is("inject") {
            let k = words.next()?;
            let kind = [Inject::Abort, Inject::Te, Inject::Modf]
                .into_iter()
                .find(|i| k.eq_ignore_ascii_case(i.name().as_bytes()))?;
            DmaVerb::Inject(kind)
        } else if is("report") {
            DmaVerb::Report
        } else if is("stop") {
            DmaVerb::Stop
        } else {
            return None;
        };
        Some(verb)
    }
}

impl fmt::Display for DmaVerb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DmaVerb::Chunk => f.write_str("chunk"),
            DmaVerb::Burst => f.write_str("burst"),
            DmaVerb::Sustain(n) => write!(f, "sustain {}", n.get()),
            DmaVerb::Stall(n) => write!(f, "stall {}", n.get()),
            DmaVerb::Ui(n) => write!(f, "ui {}", n.get()),
            DmaVerb::Back => f.write_str("back"),
            DmaVerb::Listen(n) => write!(f, "listen {}", n.get()),
            DmaVerb::Wake(n) => write!(f, "wake {}", n.get()),
            DmaVerb::Inject(i) => write!(f, "inject {}", i.name()),
            DmaVerb::Report => f.write_str("report"),
            DmaVerb::Stop => f.write_str("stop"),
        }
    }
}

/// A histogram's bucket, in µs.
pub const BUCKET_US: u32 = 100;

/// Times in `BUCKET_US` buckets, then a count of those past the last.
#[derive(Clone, Debug)]
pub struct Histogram<const BUCKETS: usize> {
    buckets: [u32; BUCKETS],
    over: u32,
}

impl<const BUCKETS: usize> Default for Histogram<BUCKETS> {
    fn default() -> Self {
        Self::new()
    }
}

/// A percentile: within a bucket's upper edge, or past the last bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Percentile {
    Within(Micros),
    Over,
}

impl fmt::Display for Percentile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Percentile::Within(us) => write!(f, "{}", us.get()),
            Percentile::Over => f.write_str("over"),
        }
    }
}

/// A run's spread, from its histogram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spread {
    pub n: u32,
    pub median: Percentile,
    pub p95: Percentile,
    pub p99: Percentile,
    pub p999: Percentile,
    pub max: Percentile,
}

impl<const BUCKETS: usize> Histogram<BUCKETS> {
    pub const fn new() -> Self {
        Histogram {
            buckets: [0; BUCKETS],
            over: 0,
        }
    }

    /// An empty one, built where it stays (10 KB at 2,500 buckets: never
    /// by value on the firmware's stack).
    pub fn init_in_place(slot: &mut core::mem::MaybeUninit<Self>) -> &mut Self {
        // SAFETY: the slot's pointer is valid for one `Self`; every field
        // is a `u32`, for which all-zero bytes are a value: an empty
        // histogram.
        unsafe {
            slot.as_mut_ptr().write_bytes(0, 1);
            slot.assume_init_mut()
        }
    }

    pub fn clear(&mut self) {
        self.buckets.fill(0);
        self.over = 0;
    }

    pub fn record(&mut self, t: Micros) {
        let i = usize::try_from(t.get() / BUCKET_US).unwrap_or(usize::MAX);
        let n = self.buckets.get_mut(i).unwrap_or(&mut self.over);
        *n = n.saturating_add(1);
    }

    pub fn count(&self) -> u32 {
        self.buckets
            .iter()
            .fold(self.over, |n, &b| n.saturating_add(b))
    }

    /// Nearest rank: the upper edge of the bucket holding it. Empty: 0.
    pub fn percentile(&self, per_mille: u16) -> Percentile {
        let n = u64::from(self.count());
        if n == 0 {
            return Percentile::Within(Micros::new(0));
        }
        let rank = (n * u64::from(per_mille.min(1_000))).div_ceil(1_000).max(1);
        let mut seen = 0u64;
        for (edge, &b) in (1u32..).zip(&self.buckets) {
            seen += u64::from(b);
            if seen >= rank {
                return Percentile::Within(Micros::new(edge * BUCKET_US));
            }
        }
        Percentile::Over
    }

    pub fn spread(&self) -> Spread {
        Spread {
            n: self.count(),
            median: self.percentile(500),
            p95: self.percentile(950),
            p99: self.percentile(990),
            p999: self.percentile(999),
            max: self.percentile(1_000),
        }
    }
}

/// `t = latency + n / bandwidth`, fitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkFit {
    pub latency: Micros,
    pub bandwidth: BytesPerSec,
}

/// Least squares of `t = L + n/BW` over `points`; `None` under two sizes
/// or for a slope at or under 0. A latency under 0 is 0.
pub fn fit_line(points: &[(Bytes, Micros)]) -> Option<LinkFit> {
    let k = i128::try_from(points.len()).ok()?;
    let (mut sx, mut sy, mut sxx, mut sxy) = (0i128, 0i128, 0i128, 0i128);
    for (b, t) in points {
        let (x, y) = (i128::from(b.get()), i128::from(t.get()));
        (sx, sy, sxx, sxy) = (sx + x, sy + y, sxx + x * x, sxy + x * y);
    }
    // k² times the variance and the covariance.
    let var = k * sxx - sx * sx;
    let cov = k * sxy - sx * sy;
    if var <= 0 || cov <= 0 {
        return None;
    }
    // µs a byte is cov / var; the mean's line through it.
    let latency = (sy * var - cov * sx) / (k * var);
    let bandwidth = var * 1_000_000 / cov;
    Some(LinkFit {
        latency: Micros::new(u32::try_from(latency.max(0)).unwrap_or(u32::MAX)),
        bandwidth: BytesPerSec::new(u32::try_from(bandwidth).unwrap_or(u32::MAX)),
    })
}

/// A streaming bench's streams, as `BENCH` counted them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamLines {
    pub n_streams: StreamCount,
    pub report: BenchReport,
    /// Fills the UI saw too late to time: the ring had moved past them.
    pub fills_lost: u32,
    /// Each bed's runs on the card: over 1, its chunks can straddle one.
    pub runs: [u32; crate::hw::MAX_VOICES],
}

/// Each verb's own lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaOwn {
    /// B2: fits over each size's p99, idle and loaded; CRC16 of a block.
    Chunk {
        idle: Option<LinkFit>,
        fit: Option<LinkFit>,
        crc16: Micros,
    },
    /// B3: the spread is the 8th read's.
    Burst,
    /// B4, B8: the spread is the fills'.
    Streams(StreamLines),
    /// B5: the spread is the writes' busy.
    Stall {
        streams: StreamLines,
        write_busy_errors: u32,
    },
    /// B6.
    Ui {
        streams: StreamLines,
        ui_fps_min: u32,
        ui_frozen_max: Millis,
        save_ok: bool,
    },
    /// B7: the spread is the backward walk's.
    Back {
        back_mean: Micros,
        forward_mean: Micros,
    },
    /// B9, B10.
    Reads {
        streams: StreamLines,
        ui_reads: u32,
        ui_reads_ok: u32,
        faults: u32,
    },
}

/// A bench's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaReport {
    pub spread: Spread,
    /// Audio blocks missed during the run.
    pub overruns: u32,
    pub loaded_load_pct: u16,
    pub card: CardKind,
    pub card_mb: u32,
    /// The card IRQ's, during the run.
    pub aborts: u32,
    pub stop_errors: u32,
    /// Since boot (M2): the card IRQ's longest entry, wall time.
    pub card_irq_us_max: Micros,
    /// Since boot: its longest entry less the audio that preempted it.
    pub card_irq_own_us_max: Micros,
    /// Its own time's share of the run (CARD IRQ, slice 1 § 10.4).
    pub card_irq_share: super::PerMille,
    pub irq: IrqCounts,
    pub own: DmaOwn,
}

/// The card IRQ's work during a run: its entries, and the bursts (a
/// `DMA2_STR0` entry each) its reads took, from CMD18 to the drain's end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IrqCounts {
    pub entries: u32,
    pub reads: u32,
    pub bursts: u32,
    pub bursts_max: u32,
}

/// A report's lines.
pub fn write_dma(r: &DmaReport, t: &mut impl Write) -> fmt::Result {
    let s = &r.spread;
    writeln!(t, "n {}", s.n)?;
    writeln!(t, "median_us {}", s.median)?;
    writeln!(t, "p95_us {}", s.p95)?;
    writeln!(t, "p99_us {}", s.p99)?;
    writeln!(t, "p999_us {}", s.p999)?;
    writeln!(t, "max_us {}", s.max)?;
    writeln!(t, "overruns {}", r.overruns)?;
    writeln!(t, "loaded_load_pct {}", r.loaded_load_pct)?;
    writeln!(t, "card {}", r.card.name())?;
    writeln!(t, "card_mb {}", r.card_mb)?;
    writeln!(
        t,
        "git {}{}",
        THIS.hash(),
        if THIS.dirty() { "+" } else { "" }
    )?;
    writeln!(t, "aborts {}", r.aborts)?;
    writeln!(t, "stop_errors {}", r.stop_errors)?;
    writeln!(t, "card_irq_us_max {}", r.card_irq_us_max.get())?;
    writeln!(t, "card_irq_own_us_max {}", r.card_irq_own_us_max.get())?;
    writeln!(t, "card_irq_pct {}", r.card_irq_share)?;
    writeln!(t, "card_irqs {}", r.irq.entries)?;
    writeln!(t, "card_reads {}", r.irq.reads)?;
    writeln!(t, "card_read_bursts {}", r.irq.bursts)?;
    writeln!(t, "card_read_bursts_max {}", r.irq.bursts_max)?;
    match &r.own {
        DmaOwn::Chunk { idle, fit, crc16 } => {
            write_fit(t, "idle_fit", idle)?;
            write_fit(t, "fit", fit)?;
            writeln!(t, "crc16_us {}", crc16.get())
        }
        DmaOwn::Burst => Ok(()),
        DmaOwn::Streams(s) => write_streams(t, s),
        DmaOwn::Stall {
            streams,
            write_busy_errors,
        } => {
            write_streams(t, streams)?;
            writeln!(t, "write_busy_errors {write_busy_errors}")
        }
        DmaOwn::Ui {
            streams,
            ui_fps_min,
            ui_frozen_max,
            save_ok,
        } => {
            write_streams(t, streams)?;
            writeln!(t, "ui_fps_min {ui_fps_min}")?;
            writeln!(t, "ui_frozen_max_ms {}", ui_frozen_max.get())?;
            writeln!(t, "save_ok {}", u8::from(*save_ok))
        }
        DmaOwn::Back {
            back_mean,
            forward_mean,
        } => {
            writeln!(t, "back_mean_us {}", back_mean.get())?;
            writeln!(t, "forward_mean_us {}", forward_mean.get())
        }
        DmaOwn::Reads {
            streams,
            ui_reads,
            ui_reads_ok,
            faults,
        } => {
            write_streams(t, streams)?;
            writeln!(t, "ui_reads {ui_reads}")?;
            writeln!(t, "ui_reads_ok {ui_reads_ok}")?;
            writeln!(t, "faults {faults}")
        }
    }
}

fn write_fit(t: &mut impl Write, name: &str, fit: &Option<LinkFit>) -> fmt::Result {
    match fit {
        None => writeln!(t, "{name} none"),
        Some(f) => {
            writeln!(t, "{name}_latency_us {}", f.latency.get())?;
            writeln!(t, "{name}_bandwidth_kbs {}", f.bandwidth.get() / 1_000)
        }
    }
}

fn write_streams(t: &mut impl Write, s: &StreamLines) -> fmt::Result {
    let r = &s.report;
    writeln!(t, "n_streams {}", s.n_streams.get())?;
    writeln!(t, "empties {}", r.empties)?;
    writeln!(t, "late_max_blocks {}", r.late_max_blocks)?;
    writeln!(t, "blocks {}", r.blocks)?;
    writeln!(t, "order_errors {}", r.order_errors)?;
    let streams = &r.check_errors[..usize::from(s.n_streams.get())];
    writeln!(t, "check_errors {}", streams.iter().sum::<u32>())?;
    for (k, n) in streams.iter().enumerate() {
        writeln!(t, "check_errors_{k} {n}")?;
    }
    writeln!(t, "fills_lost {}", s.fills_lost)?;
    for (k, n) in s.runs.iter().take(streams.len()).enumerate() {
        writeln!(t, "runs_{k} {n}")?;
    }
    Ok(())
}

/// Why a `card dma` verb didn't start or report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaRefusal {
    /// A run is going.
    Busy,
    /// No `BEDS.CHK`, a bed that isn't a `Plain` source, or fewer beds
    /// than streams.
    Beds,
    /// `report` with nothing run this boot.
    NoRun,
    /// A bench build without bank 2: no FLASH for the benches.
    Absent,
    Card(CardFail),
}

/// What a `card dma` answers, at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaAnswer {
    Started(DmaVerb),
    Stopped,
    /// Per cent done.
    Running(u8),
    Report(DmaReport),
    Refused(DmaRefusal),
}

/// The whole answer, its `OK` or `ERR` line included.
pub fn write_dma_answer(a: &DmaAnswer, t: &mut impl Write) -> fmt::Result {
    match a {
        DmaAnswer::Started(v) => writeln!(t, "OK started {v}"),
        DmaAnswer::Stopped => t.write_str("OK stopped\n"),
        DmaAnswer::Running(pct) => writeln!(t, "running {pct}\nOK"),
        DmaAnswer::Report(r) => write_dma(r, t).and_then(|()| t.write_str("OK\n")),
        DmaAnswer::Refused(DmaRefusal::Busy) => t.write_str("ERR busy\n"),
        DmaAnswer::Refused(DmaRefusal::Beds) => t.write_str("ERR beds\n"),
        DmaAnswer::Refused(DmaRefusal::NoRun) => t.write_str("ERR card: no dma run\n"),
        DmaAnswer::Refused(DmaRefusal::Absent) => {
            t.write_str("ERR card dma is not in this build\n")
        }
        DmaAnswer::Refused(DmaRefusal::Card(e)) => writeln!(t, "ERR card: {}", e.message()),
    }
}
