//! `chimera-import tape` (TAPE spec § 6): a WAV into a TAPE source, sealed
//! on the sim's card. `build` is the pure part, WAV samples to a head and
//! its packed bytes; `import` reads the file and writes through
//! `chimera-core`'s `write_source`, so the unit reads what the host wrote.

pub mod convert;
pub mod transwave;

use chimera_core::stream::units::SourceRate;
use std::fmt;
use std::ops::{ControlFlow, Range};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chimera_core::console::PUT_MAX_BYTES;
use chimera_core::dsp::clampf;
use chimera_core::dsp::tape_engine::MIN_LOOP_S;
use chimera_core::dsp::tape_engine::heads::loop_len;
use chimera_core::dsp::tape_engine::read::{MIN_SOURCE_FRAMES, Shape};
use chimera_core::factory::factory_entries;
use chimera_core::name::SoundName;
use chimera_core::project::arena::{ARENA_BYTES, Cost};
use chimera_core::project::{SOURCE_HEAD, SlotId};
use chimera_core::storage::twelve::pack12;
use chimera_core::storage::{
    CardFault, Ready, SaveError, SourceCheck, SourceHead, SourceRef, WholeShape, Wrote, held_len,
    source_crc, source_file, source_file_min_len, write_source,
};
use chimera_hal::store::{Dir, ReadSink, Store, StoreError};

use convert::{
    Channel, Mono, Pitch, Smpl, WavError, clean, quantise_tpdf, read_wav, resample, yin,
};
use transwave::{MAX_FRAMES, cut_cycles, round12, transwave};

/// The report's last line: where the source is, and how it reaches the
/// unit (ADR 0106).
pub const SEND_LINE: &str = "ON THE SIM CARD · just put-tape <FILE> SENDS IT TO THE UNIT";

/// The largest WAV the importer reads: about 25 minutes of 48 kHz 24-bit
/// stereo, far past `put`'s 1 MiB, and held four times over as it converts.
pub const MAX_WAV_BYTES: u64 = 512 << 20;

/// The dither's seed: a source rebuilds bit for bit, so a re-import is the
/// same file.
const SEED: u64 = 0x7A9E;

/// A plain source's rate (#279's S950 choices; the descriptor takes
/// 4 000–48 000 Hz).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rate {
    K48,
    K32,
    K24,
    K16,
    K12,
    K8,
}

impl Rate {
    pub const ALL: [Rate; 6] = [
        Rate::K48,
        Rate::K32,
        Rate::K24,
        Rate::K16,
        Rate::K12,
        Rate::K8,
    ];

    pub const fn hz(self) -> u16 {
        match self {
            Rate::K48 => 48_000,
            Rate::K32 => 32_000,
            Rate::K24 => 24_000,
            Rate::K16 => 16_000,
            Rate::K12 => 12_000,
            Rate::K8 => 8_000,
        }
    }
}

/// `24k`, `24` or `24000`.
impl FromStr for Rate {
    type Err = String;

    fn from_str(s: &str) -> Result<Rate, String> {
        let bad = || format!("--rate {s}: one of 48k, 32k, 24k, 16k, 12k, 8k");
        let n: u32 = s.trim_end_matches(['k', 'K']).parse().map_err(|_| bad())?;
        let hz = if n < 1_000 { n * 1_000 } else { n };
        Rate::ALL
            .into_iter()
            .find(|r| u32::from(r.hz()) == hz)
            .ok_or_else(bad)
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} kHz", self.hz() / 1_000)
    }
}

/// A root in MIDI cents, 0 to 12 700 (6 000 is C4): the head's `root`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Root(u16);

const NOTES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

impl Root {
    pub const C4: Root = Root(6_000);

    pub const fn new(cents: u16) -> Option<Root> {
        if cents <= 12_700 {
            Some(Root(cents))
        } else {
            None
        }
    }

    pub const fn cents(self) -> u16 {
        self.0
    }
}

/// A note name, `C-1` to `G9`: `C3`, `F#2`, `Bb4`.
impl FromStr for Root {
    type Err = String;

    fn from_str(s: &str) -> Result<Root, String> {
        let bad = || format!("--root {s}: a note, C-1 to G9 (C4 is middle C)");
        let mut chars = s.chars();
        let letter = chars.next().ok_or_else(bad)?.to_ascii_uppercase();
        let pc = NOTES
            .iter()
            .position(|n| n.starts_with(letter) && n.len() == 1);
        let mut rest = chars.as_str();
        let shift = match rest.chars().next() {
            Some('#') => 1,
            Some('b') => -1,
            _ => 0,
        };
        if shift != 0 {
            rest = &rest[1..];
        }
        let octave: i32 = rest.parse().map_err(|_| bad())?;
        let note = (octave + 1) * 12 + pc.ok_or_else(bad)? as i32 + shift;
        u16::try_from(note * 100)
            .ok()
            .and_then(Root::new)
            .ok_or_else(bad)
    }
}

/// The nearest note, and the cents off it: `A4`, `A#2 +12`.
impl fmt::Display for Root {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let note = (self.0 + 50) / 100;
        let off = i32::from(self.0) - i32::from(note) * 100;
        write!(
            f,
            "{}{}",
            NOTES[note as usize % 12],
            i32::from(note / 12) - 1
        )?;
        if off != 0 {
            write!(f, " {off:+}")?;
        }
        Ok(())
    }
}

/// `--transwave`'s cycle, in input samples: finite, at least 2.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cycle(f32);

impl Cycle {
    pub fn new(samples: f32) -> Option<Cycle> {
        (samples.is_finite() && samples >= 2.0).then_some(Cycle(samples))
    }

    pub fn samples(self) -> f32 {
        self.0
    }
}

/// A transwave's stored top level: 512 by default, 1 024 with `--top 1024`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Top {
    T512,
    T1024,
}

impl Top {
    pub const fn samples(self) -> u32 {
        match self {
            Top::T512 => 512,
            Top::T1024 => 1_024,
        }
    }
}

/// What the import makes. A transwave is 48 kHz nominal and plays its
/// cycle at the note's frequency, so it takes no rate and no root.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Form {
    Plain {
        rate: Rate,
        root: Option<Root>,
    },
    /// At most `frames` cycles (1–32) cut from across the take.
    Transwave {
        cycle: Cycle,
        top: Top,
        frames: u8,
    },
}

/// `just tape`'s arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct TapeOpts {
    pub wav: PathBuf,
    pub form: Form,
    /// The file's stem when `None`.
    pub name: Option<SoundName>,
    pub channel: Channel,
}

impl TapeOpts {
    /// `<file.wav> [--rate 24k] [--root C3] [--transwave <cycle> [--top
    /// 1024]] [--name CHOIR] [--channel mix|L|R]`.
    pub fn from_args(args: &[String]) -> Result<TapeOpts, String> {
        let (mut wav, mut flags) = (None, Vec::<(&str, &str)>::new());
        let mut it = args.iter().map(String::as_str);
        while let Some(a) = it.next() {
            if let Some(flag) = a.strip_prefix("--") {
                let v = it.next().ok_or(format!("--{flag} needs a value"))?;
                if flags.iter().any(|&(f, _)| f == flag) {
                    return Err(format!("--{flag} given twice"));
                }
                flags.push((flag, v));
            } else if wav.is_some() || a.is_empty() {
                return Err(format!("one WAV, not {a:?}"));
            } else {
                wav = Some(PathBuf::from(a));
            }
        }
        let get = |f: &str| flags.iter().find(|&&(g, _)| g == f).map(|&(_, v)| v);
        if let Some(&(f, _)) = flags.iter().find(|(f, _)| {
            ![
                "rate",
                "root",
                "transwave",
                "top",
                "frames",
                "name",
                "channel",
            ]
            .contains(f)
        }) {
            return Err(format!("--{f}: no such option"));
        }
        let form = match get("transwave") {
            Some(c) => {
                if get("rate").is_some() || get("root").is_some() {
                    return Err("--transwave takes no --rate or --root: it is 48 kHz \
                                nominal, and the note sets its pitch"
                        .into());
                }
                let cycle = c.parse().ok().and_then(Cycle::new).ok_or(format!(
                    "--transwave {c}: a cycle of 2 input samples or more"
                ))?;
                let top = match get("top") {
                    None | Some("512") => Top::T512,
                    Some("1024") => Top::T1024,
                    Some(t) => return Err(format!("--top {t}: 512 or 1024")),
                };
                let frames = match get("frames").map(str::parse::<u8>) {
                    None => MAX_FRAMES as u8,
                    Some(Ok(n)) if (1..=MAX_FRAMES as u8).contains(&n) => n,
                    Some(_) => return Err("--frames: 1 to 32".into()),
                };
                Form::Transwave { cycle, top, frames }
            }
            None if get("top").is_some() || get("frames").is_some() => {
                return Err("--top and --frames are a --transwave's".into());
            }
            None => Form::Plain {
                rate: get("rate").map_or(Ok(Rate::K24), str::parse)?,
                root: get("root").map(str::parse).transpose()?,
            },
        };
        let name = get("name")
            .map(|n| SoundName::new(&n.to_ascii_uppercase()))
            .transpose()
            .map_err(|e| format!("--name: {e:?} (16 of A-Z 0-9, space, -)"))?;
        let channel = match get("channel").map(str::to_ascii_lowercase).as_deref() {
            None | Some("mix") => Channel::Mix,
            Some("l") => Channel::L,
            Some("r") => Channel::R,
            Some(c) => return Err(format!("--channel {c}: mix, L or R")),
        };
        Ok(TapeOpts {
            wav: wav.ok_or("no WAV")?,
            form,
            name,
            channel,
        })
    }
}

/// How a source is tuned, as its head's labels say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tune {
    /// YIN's period, in frames at the source's rate.
    Pitched { root: Root, period: f32 },
    /// No period (`period` 0): STRETCH's MONO plays it as POLY.
    Unpitched { root: Root },
    /// A transwave: its cycle plays at the note's frequency; rate and
    /// root don't enter.
    Note,
}

impl Tune {
    /// What `h`'s labels say: a transwave's are none.
    pub fn of(h: &SourceHead) -> Tune {
        let root = Root(h.root.min(12_700));
        match (h.shape, h.period_q16) {
            (Shape::Transwave, _) => Tune::Note,
            (_, 0) => Tune::Unpitched { root },
            (_, q) => Tune::Pitched {
                root,
                period: q as f32 / 65_536.0,
            },
        }
    }
}

/// A `smpl` loop as LOOP's macros: `Bounds::of` with these over the whole
/// source plays it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Suggest {
    pub position: f32,
    pub size: f32,
}

/// What `build` makes: the head and samples `write_source` takes, and a
/// `smpl` loop's suggestion.
pub struct Built {
    pub head: SourceHead,
    pub packed: Vec<u8>,
    pub suggest: Option<Suggest>,
}

/// What `seal` heads: a plain source with its labels (`count` 1), or a
/// transwave of `count` frames at 48 kHz nominal, with none.
enum Whole {
    Plain {
        rate: Rate,
        root: Root,
        /// YIN's, in frames; `None` is unpitched.
        period: Option<f32>,
    },
    Transwave {
        count: u8,
    },
}

#[derive(Debug)]
pub enum ImportError {
    /// The WAV couldn't be read from disk.
    Io(std::io::Error),
    Wav(WavError),
    /// Nothing over −180 dBFS, or a transwave's cycles all silent.
    Silent,
    /// Under `MIN_SOURCE_FRAMES` at the rate.
    TooShort {
        frames: usize,
    },
    /// A resample `rubato` refused.
    Resample,
    /// `--transwave`'s cycle doesn't fit the take whole.
    NoCycles,
    /// A file past `put`'s 1 MiB: nothing written.
    TooBig {
        bytes: u64,
    },
    /// A WAV past `MAX_WAV_BYTES`, refused before it is read.
    WavTooBig {
        bytes: u64,
    },
    /// The card refused the write, or a sealed copy is damaged.
    Save(SaveError),
}

impl From<StoreError> for ImportError {
    fn from(e: StoreError) -> Self {
        ImportError::Save(SaveError::Store(e))
    }
}

impl CardFault for ImportError {
    fn store_error(&self) -> Option<StoreError> {
        match self {
            ImportError::Save(e) => e.store_error(),
            _ => None,
        }
    }
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ImportError::Io(e) => write!(f, "can't read the WAV: {e}"),
            ImportError::Wav(e) => write!(f, "not a WAV it reads: {e:?}"),
            ImportError::Silent => f.write_str("no audio: empty or silent"),
            ImportError::TooShort { frames } => write!(
                f,
                "too short: {frames} frames at the rate, a source needs {MIN_SOURCE_FRAMES}"
            ),
            ImportError::WavTooBig { bytes } => write!(
                f,
                "a {bytes} B WAV: past the {MAX_WAV_BYTES} B the importer reads"
            ),
            ImportError::Resample => f.write_str("can't resample from the file's rate"),
            ImportError::NoCycles => f.write_str("--transwave: not one whole cycle in the take"),
            ImportError::TooBig { bytes } => write!(
                f,
                "{bytes} B: past put's {PUT_MAX_BYTES} B; a lower --rate or a shorter take"
            ),
            ImportError::Save(SaveError::Damaged) => f.write_str(
                "a sealed copy on the card fails its check: remove it by hand, then import again",
            ),
            ImportError::Save(e) => write!(f, "the card refused it: {e:?}"),
        }
    }
}

/// What NEW's Sounds leave the arena: what a source must fit beside. Spec
/// § 4 budgets 153 600 B for 32 NEW Sounds; NEW holds fewer today, and this
/// follows it.
pub fn free_beside_new() -> Cost {
    let sounds: usize = SlotId::ALL
        .into_iter()
        .zip(factory_entries())
        .map(|(_, (_, b))| Cost::of(b.len()).bytes() as usize)
        .sum();
    Cost::of(ARENA_BYTES - sounds)
}

/// What an import wrote, and what to tell the user of it: the head as the
/// card holds it (on `Present`, the first import's labels, not this one's).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Report {
    pub head: SourceHead,
    pub suggest: Option<Suggest>,
    pub wrote: Wrote,
}

impl Report {
    pub fn tune(&self) -> Tune {
        Tune::of(&self.head)
    }

    /// A transwave's frames (single cycles); `None` for a plain source.
    pub fn cycles(&self) -> Option<u8> {
        (self.head.shape == Shape::Transwave).then_some(self.head.count)
    }

    pub fn seconds(&self) -> f32 {
        self.head.sref.frames as f32 / f32::from(self.head.sref.rate)
    }

    /// On the meter: `16 + held`, to the granule.
    pub fn cost(&self) -> Cost {
        let held = held_len(self.head.sref).expect("a source under put's 1 MiB");
        Cost::of(SOURCE_HEAD + held)
    }

    /// Past what NEW leaves: a project of NEW can't load it.
    pub fn wont_fit(&self) -> bool {
        self.cost() > free_beside_new()
    }

    /// Its card path: `/CHIMERA/TAPES/<CRC>.SRC`.
    pub fn file(&self) -> String {
        let f = source_file(self.head.sref);
        let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
        format!("/CHIMERA/TAPES/{}.{}", s(f.stem()), s(f.ext()))
    }
}

/// One field a line, as spec § 6 step 8 lists them, then `SEND_LINE`.
impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "NAME     {}", self.head.name.as_str())?;
        let tune = self.tune();
        match tune {
            Tune::Note => writeln!(f, "RATE     48 kHz NOMINAL (TRANSWAVE)")?,
            _ => writeln!(f, "RATE     {} kHz", self.head.sref.rate / 1_000)?,
        }
        writeln!(f, "FRAMES   {}", self.head.sref.frames)?;
        if let Some(n) = self.cycles() {
            writeln!(f, "CYCLES   {n}")?;
        }
        match tune {
            Tune::Note => writeln!(f, "SECONDS  NONE: A TRANSWAVE IS CYCLES, NOT TIME")?,
            _ => writeln!(f, "SECONDS  {:.2}", self.seconds())?,
        }
        match tune {
            Tune::Pitched { root, period } => {
                writeln!(f, "ROOT     {root}")?;
                writeln!(f, "PERIOD   {period:.3} FRAMES")?;
            }
            Tune::Unpitched { root } => {
                writeln!(f, "ROOT     {root}")?;
                writeln!(f, "PERIOD   UNPITCHED")?;
            }
            Tune::Note => {
                writeln!(f, "ROOT     THE NOTE'S")?;
                writeln!(f, "PERIOD   NONE: A TRANSWAVE PLAYS ITS CYCLE AT THE NOTE")?;
            }
        }
        writeln!(
            f,
            "COST     {} ON THE METER",
            self.cost().kib_label().as_str()
        )?;
        if let Some(s) = self.suggest {
            writeln!(
                f,
                "LOOP     POSITION {:.3} · SIZE {:.3} (THE FILE'S smpl LOOP)",
                s.position, s.size
            )?;
        }
        if self.wont_fit() {
            writeln!(
                f,
                "WARNING  WON'T FIT BESIDE NEW · NEEDS {}, {} FREE",
                self.cost().kib_label().as_str(),
                free_beside_new().kib_floor_label().as_str()
            )?;
        }
        let wrote = match self.wrote {
            Wrote::New => "NEW",
            Wrote::Present => "PRESENT: THE SAME AUDIO WAS ON THE CARD; ITS LABELS STAY",
            Wrote::Replaced => "REPLACED A TORN COPY",
        };
        writeln!(f, "WROTE    {wrote}")?;
        writeln!(f, "FILE     {}", self.file())?;
        writeln!(f, "{SEND_LINE}")
    }
}

/// `m` as a source (spec § 6 steps 2–6): cleaned; a plain source
/// resampled, pitched and dithered to 12 bits, a transwave's cycles cut,
/// built into levels and rounded; packed, its CRC its stem. Past `put`'s
/// 1 MiB is `TooBig`.
pub fn build(mut m: Mono, form: Form, name: SoundName) -> Result<Built, ImportError> {
    let kept = clean(&mut m);
    if m.samples.is_empty() {
        return Err(ImportError::Silent);
    }
    match form {
        Form::Plain { rate, root } => plain(&m, kept, rate, root, name),
        Form::Transwave { cycle, top, frames } => {
            let cycles = cut_cycles(&m.samples, cycle.samples(), frames.into());
            if cycles.is_empty() {
                return Err(ImportError::NoCycles);
            }
            let q = round12(&transwave(&cycles, top.samples())).ok_or(ImportError::Silent)?;
            let (pairs, []) = q.as_chunks::<2>() else {
                unreachable!("every level is an even length")
            };
            let count = u8::try_from(cycles.len()).expect("at most 32 frames");
            seal(Whole::Transwave { count }, name, pairs)
        }
    }
}

fn plain(
    m: &Mono,
    kept: Range<usize>,
    rate: Rate,
    root: Option<Root>,
    name: SoundName,
) -> Result<Built, ImportError> {
    let hz = rate.hz();
    let x = resample(&m.samples, m.rate, hz.into()).map_err(|_| ImportError::Resample)?;
    if x.len() < MIN_SOURCE_FRAMES as usize {
        return Err(ImportError::TooShort { frames: x.len() });
    }
    // The root: `--root`, the `smpl` chunk's, YIN's, else C4.
    let root = root.or_else(|| m.smpl.and_then(smpl_root));
    let (root, period) = match yin(&x, hz.into()) {
        Pitch::Pitched { period, root_cents } => (
            root.unwrap_or(Root::new(root_cents).unwrap_or(Root::C4)),
            Some(period),
        ),
        Pitch::Unpitched => (root.unwrap_or(Root::C4), None),
    };
    let q = quantise_tpdf(&x, SEED);
    let (pairs, []) = q.as_chunks::<2>() else {
        unreachable!("quantise_tpdf holds whole pairs")
    };
    let whole = Whole::Plain { rate, root, period };
    let built = seal(whole, name, pairs)?;
    let suggest = m
        .smpl
        .and_then(|s| s.loop_frames)
        .and_then(|lp| suggest(lp, kept, x.len(), built.head.sref));
    Ok(Built { suggest, ..built })
}

/// The `smpl` chunk's unity note and fraction, in cents.
fn smpl_root(s: Smpl) -> Option<Root> {
    let fraction = (f64::from(s.fraction) / 4_294_967_296.0 * 100.0).round() as u16;
    Root::new((u16::from(s.unity?) * 100 + fraction).min(12_700))
}

/// `pairs` packed under `whole`'s head, its CRC over the bytes. Refused
/// past `put`'s 1 MiB.
fn seal(whole: Whole, name: SoundName, pairs: &[[i16; 2]]) -> Result<Built, ImportError> {
    let (shape, count, rate, root, period_q16) = match whole {
        Whole::Plain { rate, root, period } => (
            WholeShape::Plain,
            1,
            rate.hz(),
            root.cents(),
            period.map_or(0, |p| (f64::from(p) * 65_536.0).round() as u32),
        ),
        Whole::Transwave { count } => (WholeShape::Transwave, count, 48_000, Root::C4.cents(), 0),
    };
    let too_big = |bytes| ImportError::TooBig { bytes };
    let frames = u32::try_from(2 * pairs.len()).map_err(|_| too_big(u64::MAX))?;
    let unsealed = SourceRef {
        crc: 0,
        frames,
        rate,
    };
    let bytes = source_file_min_len(unsealed).map_or(u64::MAX, u64::from);
    if bytes > u64::from(PUT_MAX_BYTES) {
        return Err(too_big(bytes));
    }
    let mut packed = vec![0; 3 * pairs.len()];
    pack12(pairs.as_flattened(), &mut packed);
    let head = SourceHead {
        sref: SourceRef {
            crc: source_crc(shape, count, &packed),
            ..unsealed
        },
        shape: shape.into(),
        count,
        root,
        period_q16,
        name,
    };
    Ok(Built {
        head,
        packed,
        suggest: None,
    })
}

/// A `smpl` loop `lp` (first and last frames, inclusive, of the input) as
/// LOOP's POSITION and SIZE over `s`, its `out` resampled frames from the
/// `kept` input frames: SIZE inverts `loop_len` (`2 ms · (W / 2 ms)^SIZE`),
/// POSITION places the start in the room the loop leaves. `None` for a
/// loop the trim cut away.
///
/// It inverts the floor at the source's own speed (2 ms, `loop_len`). The
/// engine floors a loop by `shortest(rate, inc)`, at least 16 output samples
/// of the head's travel, so a loop under that floor plays longer than
/// suggested on high notes: at 4× speed, any loop under 64 frames.
fn suggest(lp: (u32, u32), kept: Range<usize>, out: usize, s: SourceRef) -> Option<Suggest> {
    let k = kept.len() as f64;
    let from = (f64::from(lp.0) - kept.start as f64).max(0.0);
    let to = (f64::from(lp.1) + 1.0 - kept.start as f64).min(k);
    if to <= from {
        return None;
    }
    let scale = out as f64 / k;
    let (ls, ll, w) = (from * scale, (to - from) * scale, f64::from(s.frames));
    let min = f64::from(MIN_LOOP_S) * f64::from(s.rate);
    let size = if w > min {
        clampf(((ll.max(min) / min).ln() / (w / min).ln()) as f32, 0.0, 1.0)
    } else {
        1.0
    };
    let played = f64::from(loop_len(size, s.frames, SourceRate::new(s.rate)?));
    let position = if w > played {
        clampf((ls / (w - played)) as f32, 0.0, 1.0)
    } else {
        0.0
    };
    Some(Suggest { position, size })
}

/// A name from the file's stem: upper case, anything a name can't hold a
/// space, at most 16; `TAPE` if nothing is left.
pub fn name_of(wav: &Path) -> SoundName {
    let stem = wav
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    let clean: String = stem
        .chars()
        .map(|c| match c.to_ascii_uppercase() {
            c @ ('A'..='Z' | '0'..='9' | '-') => c,
            _ => ' ',
        })
        .collect();
    let words = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = words.chars().take(16).collect();
    SoundName::new(cut.trim_end()).unwrap_or(SoundName::new("TAPE").expect("a name"))
}

/// `o.wav` to a sealed source on the card `r` lends (spec § 6 steps 1–8):
/// `write_source` writes it, reads it back and seals it; a sealed copy of
/// the same audio is `Present`, nothing written, and the report gives the
/// card's head. A WAV past `MAX_WAV_BYTES` is refused unread.
pub fn import<S: Store>(o: &TapeOpts, s: &mut S, r: &Ready) -> Result<Report, ImportError> {
    let bytes = std::fs::metadata(&o.wav).map_err(ImportError::Io)?.len();
    if bytes > MAX_WAV_BYTES {
        return Err(ImportError::WavTooBig { bytes });
    }
    let bytes = std::fs::read(&o.wav).map_err(ImportError::Io)?;
    let m = read_wav(&bytes, o.channel).map_err(ImportError::Wav)?;
    let name = o.name.unwrap_or_else(|| name_of(&o.wav));
    let b = build(m, o.form, name)?;
    let wrote = write_source(s, r, &b.head, &b.packed).map_err(ImportError::Save)?;
    let head = match wrote {
        Wrote::Present => card_head(s, r, b.head.sref)?,
        Wrote::New | Wrote::Replaced => b.head,
    };
    Ok(Report {
        head,
        suggest: b.suggest,
        wrote,
    })
}

/// `sref`'s head as the card holds it, read through `put`'s own check. A
/// file that fails it is `Damaged`.
fn card_head<S: Store>(s: &mut S, r: &Ready, sref: SourceRef) -> Result<SourceHead, ImportError> {
    struct Feed(SourceCheck);
    impl ReadSink for Feed {
        fn begin(&mut self, _: u32) -> ControlFlow<()> {
            ControlFlow::Continue(())
        }

        fn chunk(&mut self, b: &[u8]) -> ControlFlow<()> {
            match self.0.feed(b) {
                Ok(()) => ControlFlow::Continue(()),
                Err(_) => ControlFlow::Break(()),
            }
        }
    }
    let (vol, file) = (r.volume(), source_file(sref));
    let mut len = None;
    s.list(vol, Dir::Tapes, &mut |n, size| {
        if n == file {
            len = Some(size)
        }
    })?;
    let damaged = |_| ImportError::Save(SaveError::Damaged);
    let check = SourceCheck::new(sref.stem(), len.ok_or(StoreError::NotFound)?).map_err(damaged)?;
    let mut feed = Feed(check);
    s.read(vol, file, &mut feed)?;
    feed.0.finish().map_err(damaged)
}
